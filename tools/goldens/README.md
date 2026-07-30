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
case-sensitive, and formatter extensions are enabled. Every case is rendered with
`CultureInfo.InvariantCulture`.

Build warnings would land on stdout and corrupt the JSON, so the project treats warnings
as errors.

## Output shape

```json
{
  "smartformat_net_version": "3.6.1",
  "culture": "InvariantCulture",
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

A case that throws carries `{"error": "<exception type name>"}` instead of `result`. The
per-case `culture` field is always `""` (invariant) for now; it exists so culture-specific
cases can be added without changing the schema.

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

Doubles are always written with a `.` or an exponent, so `-0.0` appears as `-0.0` and a
reader can tell it apart from an integer. NaN and the infinities have no JSON number form,
which is why they need the `$f` marker.

## Coverage

The case table in `Program.cs` is grouped by feature: literals and escaping, selectors,
alignment, nesting, numeric specifiers, date specifiers, errors, `StringSource` selector
methods, formatter names and options, the list-index operator, and non-default settings.
Numeric and date cases are generated combinatorially from a value list crossed with a
specifier list, which is where most of the volume comes from.

Four areas are deliberately left out, because they belong to later milestones or fall
outside the port's scope: custom numeric and date patterns, lists as values to format, the
`choose` / `plural` / `conditional` formatters, and non-invariant cultures.
