# Golden-output harness

This console app renders a fixed table of templates with the real
[SmartFormat.NET](https://www.nuget.org/packages/SmartFormat.NET) library and prints the
results as JSON. The Rust port is tested against that JSON, so whatever .NET actually
does, including its rounding quirks and its exceptions, is what we have to match.

Regenerate the goldens from the repository root:

```sh
dotnet run --project tools/goldens > goldens/m1.json
```

The output is deterministic, so an unexpected diff means either the case table or the
pinned library version changed. Commit the regenerated file together with the change that
caused it.

## Pinned version

SmartFormat.NET **3.6.1**, from `tools/goldens/goldens.csproj`. Bump it there, rerun the
command above, and review the diff: it shows exactly which behavior the upgrade changed.

A case without a `settings` object uses `Smart.CreateDefaultSmartFormat()` with default
settings, so both the parser and the formatter throw on errors, selectors are
case-sensitive, and formatter extensions are enabled. A case without a `culture` is
rendered with `CultureInfo.InvariantCulture`.

Build warnings would land on stdout and corrupt the JSON, so the project treats warnings
as errors. `InvariantGlobalization` is set to `false` explicitly: it is already the
default, but an environment that turned it on would quietly render every culture case as
the invariant culture instead of failing.

## Output shape

```json
{
  "smartformat_net_version": "3.6.1",
  "default_culture": "InvariantCulture",
  "cases": [
    {
      "id": "num-double-2_675-F",
      "template": "{0:F}",
      "args": [2.675],
      "culture": "",
      "expected": { "result": "2.67" }
    }
  ]
}
```

A case that throws carries `{"error": "<exception type name>"}` instead of `result`.

The per-case `culture` field is the name passed to `CultureInfo.GetCultureInfo` and used as
the `IFormatProvider` of the `Format` call; `""` means `CultureInfo.InvariantCulture`. The
Rust runner resolves the same name through `fmt::culture::get`, and **fails** if the
generated table does not carry it — a missing culture must never look like a pass. The
document-level `default_culture` field names what a case without one uses.

Culture data and goldens are one artifact in two files: `goldens/m1.json` and
`crates/smartformat/src/fmt/culture/generated.rs` (see `tools/culturegen/README.md`).
On Unix .NET formats through the *system* ICU, so regenerate both with the same SDK on the
same machine in the same commit — otherwise the expected output and the culture data come
from different ICU versions and the failures look like port bugs.

## Non-default settings

A case that needs settings other than the .NET defaults carries a `settings` object
holding only the properties that differ. The Rust runner mirrors the same keys.

| Key | .NET property | Values |
| --- | --- | --- |
| `formatErrorAction` | `SmartSettings.Formatter.ErrorAction` | `Ignore`, `MaintainTokens`, `OutputErrorInResult` |
| `parseErrorAction` | `SmartSettings.Parser.ErrorAction` | same three |
| `caseSensitivity` | `SmartSettings.CaseSensitivity` | `CaseInsensitive` |
| `stringFormatCompatibility` | `SmartSettings.StringFormatCompatibility` | `true` |
| `alignmentFillCharacter` | `SmartSettings.Formatter.AlignmentFillCharacter` | a one-character string |
| `customSelectorChars` | `ParserSettings.AddCustomSelectorChars` | the characters to allow |

## How `args` maps to .NET values

The Rust runner mirrors this mapping, so keep the two in step.

| JSON | .NET |
| --- | --- |
| object at the top level | `Dictionary<string, object?>` passed as the single format argument |
| array at the top level | positional arguments (`object?[]`) |
| nested object | nested `Dictionary<string, object?>` |
| integer literal (no `.`, `e`, `E`) | `long` |
| number with a fraction or exponent | `double`, written round-trippably |
| string | `string` |
| `true` / `false` | `bool` |
| `null` | `null` |
| `{"$dt": "2009-06-15T13:45:30.0000000"}` | `DateTime`, parsed with the `"O"` round-trip format, `Unspecified` kind |
| `{"$f": "NaN" \| "Infinity" \| "-Infinity"}` | `double` |
| `{"$i32": "-255"}` | `int` (32-bit), for the cases pinning .NET's per-type integer width |

Doubles are always written with a `.` or an exponent, so `-0.0` appears as `-0.0` and a
reader can tell it apart from an integer. NaN and the infinities have no JSON number form,
which is why they need the `$f` marker. A plain integer literal is a `long`; `$i32` asks
for an `int` instead, which only matters for `X` and `B`, where .NET renders the CLR
type's own width.

## Coverage

The case table in `Program.cs` is grouped by feature: literals and escaping, selectors,
alignment, nesting, numeric specifiers, date specifiers, errors, `StringSource` selector
methods, formatter names and options, the list-index operator, non-default settings, the
`plural` / `choose` / `cond` formatters, which of the two auto-detecting formatters claims
an unnamed `|`-separated format, and the culture data. Numeric, date, plural and culture
cases are generated combinatorially from a value or culture list crossed with a specifier
list, which is where most of the volume comes from.

The culture groups cross every culture in the generated table with the specifiers whose
output is pure culture data — `N`, `C` in both signs, `P1`, and the `d` / `D` / `t` / `T` /
`f` date patterns — because that data is exactly what no reviewer can check by eye: which
of U+0020, U+00A0 and U+202F a culture groups digits with, whether its negative sign is a
hyphen or U+2212, which of the 17 currency-pattern arms it lands on, and whether its long
date name is genitive.

The `errtext-*` group renders M2 errors with `FormatErrorAction.OutputErrorInResult`, which
is the only way a case can observe an error's *text* rather than just its exception type.
It pins both shapes .NET produces there: a `FormattingException`'s own message, which
quotes the template and points a caret at the failure, and the bare message of any other
exception the evaluator wraps.

Some cases exist only to pin .NET behavior the port deliberately does not match. They are
in the table like any other case, and the Rust runner names each of them in its `SKIPPED`
list with the reason; every entry under "Known divergences" in `DESIGN.md` points at one of
those ids or at a unit test.

Three areas are deliberately left out, because they belong to later milestones or fall
outside the port's scope: custom numeric and date patterns, lists as values to format, and
the M3/M4 formatters.

Two kinds of case must **not** go in the table, because their .NET answer depends on the
machine that regenerates them rather than on the library:

- float or decimal values as `choose` options (`{0:choose(1.5|2.5):a|b}`). `ChooseFormatter`
  stringifies the value with the *thread* culture, ignoring the `IFormatProvider` of the
  call, so the case renders — or throws — differently per locale.
- date conditions (`{0:cond:Past|Present|Future}`). `ConditionalFormatter` compares against
  `DateTime.UtcNow`; there is no way to pin a wall clock from the harness. The Rust side
  takes its comparison point from `ConditionalFormatter::with_now` and is unit-tested
  instead.
