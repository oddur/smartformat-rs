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

It also renders cases it did not write, which is how the fuzzer asks .NET what a generated
template renders to: see "Rendering cases from a file" below.

## Pinned version

SmartFormat.NET **3.6.1** and SmartFormat.Extensions.Time **3.6.1**, from
`tools/goldens/goldens.csproj`. Bump them there, rerun the command above, and review the
diff: it shows exactly which behavior the upgrade changed. (The metapackage already pulls
the time extension in; it is named explicitly so the version the `time-*` cases were
generated against is written down.)

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
  "now": "2026-07-31T12:00:00.0000000",
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

## Rendering cases from a file

The harness also renders cases it did not write, which is how a fuzzer asks .NET what a
generated template renders to:

```sh
dotnet run --project tools/goldens -- --cases batch.json > rendered.json
dotnet run --project tools/goldens -- --cases -  < batch.json > rendered.json
```

The input is a document of the shape above with each case's `expected` left off, and the
output is that same document with `expected` filled in. Everything else is what a case of
the built-in table gets: the same `SmartFormatter` construction, the same per-case
settings, culture and fixture registration (templates, localization provider, variable
groups), the same pinned clock and invariant thread culture, and the same argument
mapping.

Only `template` is required of a case. `args` defaults to no arguments and an explicit
`null` is one null argument; `culture` defaults to `""`; `id` defaults to `case-<position>`
and need not be unique. An `expected` that is already there is ignored and overwritten,
which makes `--cases goldens/m1.json` re-render the golden file — it writes it back byte
for byte, and that is the check that this mode is the same renderer. The document's own
header fields are ignored on input; the output always carries the harness's own.

A case that fails is that one case's failure: whatever the render throws — including a
culture name that does not resolve, a `SplitChar` the library rejects, or a regex that
times out — becomes its `{"error": …}`, and the rest of the batch renders. Between cases
the harness also puts `ListFormatter.CollectionIndex` back to -1, so a case whose list
iteration failed part-way cannot leak its index into the cases after it (see the last
section for what that leak is; the built-in table keeps such a case last instead, so its
output does not change).

Anything wrong with the *document* is not: an unreadable file, invalid JSON, no `cases`
array, a case without a `template`, an unknown field, an unknown settings key or an
unknown settings value writes a message to stderr, writes nothing to stdout, and exits
**2**.

Three things can still take the whole run down, so a caller has to handle a batch that
never arrives:

- a template nested deeply enough to overflow the stack. `StackOverflowException` cannot
  be caught in .NET: the process dies on the spot and stdout holds nothing. Rendering runs
  on a thread with a 64 MB stack, which only moves the cliff — it sits somewhere between
  30,000 and 50,000 levels of `{0:{0:…}}` on the machine this was measured on.
- an endless loop inside the library. A runaway regex is covered — the process sets the
  default `Regex` match timeout to two seconds, so a catastrophic `ismatch` pattern reports
  a timeout instead of never returning — but nothing else is, so put a wall-clock timeout
  on the process.
- `args` nested deeper than 64 levels, which is System.Text.Json's default reader limit:
  the document is rejected as a whole (exit 2) rather than the case.

`dotnet run` writes build output to stdout, which would corrupt the JSON, so a caller that
runs the harness repeatedly should build once and execute
`tools/goldens/bin/<configuration>/net10.0/goldens` directly, or pass `--no-build`.

## The pinned clock

Two things read a wall clock: `TimeFormatter` on a `DateTime` value, and
`ConditionalFormatter`'s date branch. Both go through `SystemTime.Now()`, a settable
`Func<DateTime>`, which the harness pins with `SystemTime.SetDateTime` before it renders
anything. The instant is in the document's `now` field, and the Rust runner puts it in
`SmartSettings::now` — the port's stand-in — for **every** case, so a case that reads a
clock is as deterministic as one that does not.

`now` and every `$dt` argument have `DateTimeKind.Unspecified`, which .NET treats as
local, so the `ToUniversalTime()` calls in both extensions shift the value and the clock
by the same offset and cancel out. Two rules follow for new cases, and neither is checked
automatically:

- keep a `conddate-*` value within a couple of hours of `now` (which is local noon) or a
  whole day away from it. The three-part form compares the two *UTC* dates, and the port
  compares civil dates: the two answers agree as long as the machine's offset does not
  push a value across midnight, which for a value two hours from noon means any offset up
  to ±10 hours.
- keep `now` and any nearby value on the same side of a daylight-saving transition, which
  is why `now` is in July.

## Extensions that are not in `CreateDefaultSmartFormat`

`TimeFormatter` and `LocalizationFormatter` are added to every formatter the harness
builds. Neither can auto-detect, so only a `{…:time:…}` or `{…:L:…}` placeholder reaches
them and no other case changes. `AddExtensions` slots each one where
`WellKnownExtensionTypes` ranks it, which is what `FormatterRegistry::add` does on the
Rust side.

The localization provider is `LocalizationFixture` in `Program.cs`: a table keyed by
culture name, looked up along `CultureInfo.Parent` — specific culture → parent →
invariant, which for `zh-CN` goes through the script culture `zh-Hans`.
It is *not* the resx-backed `SmartFormat.Utilities.LocalizationProvider` — the table has
to be in source, because `localization_fixture` in the Rust runner mirrors it entry for
entry. Its two knobs are, though: the `localization` settings key picks a fixture with
`FallbackCulture` set (to `de`) or with `ReturnNameIfNotFound`, each applied exactly where
`LocalizationProvider.GetString` applies it — the requested culture's chain first, then
the fallback culture's, then the name itself.

A variables source is per case instead, under the `variables` settings key, because it is
ranked ahead of every other source: a fixture holding a group named `Length` would answer
`{0.Length}` on a string for every case in the table. `VariablesFixture` builds the three
named sets and `variables_fixture` in the Rust runner mirrors them.

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
| `convertCharacterStringLiterals` | `SmartSettings.Parser.ConvertCharacterStringLiterals` | `false` |

The same object also carries the configuration of the formatter *extensions* that have
any. These are not `SmartSettings`, but they select the formatter a case runs with in
exactly the same way, so they ride along in the same record and the same JSON object.

| Key | .NET property | Values |
| --- | --- | --- |
| `regexOptions` | `IsMatchFormatter.RegexOptions` | a `[Flags]` name, or several comma-separated |
| `isMatchSplitChar` | `IsMatchFormatter.SplitChar` | a one-character string |
| `isMatchPlaceholderName` | `IsMatchFormatter.PlaceholderNameForMatches` | the name, default `m` |
| `isMatchCanAutoDetect` | `IsMatchFormatter.CanAutoDetect` | `true` |
| `subStringOutOfRangeBehavior` | `SubStringFormatter.OutOfRangeBehavior` | `ReturnStartIndexToEndOfString`, `ThrowException` |
| `subStringNullDisplayString` | `SubStringFormatter.NullDisplayString` | the text a null value writes |
| `subStringSplitChar` | `SubStringFormatter.SplitChar` | one of `|`, `,`, `~` |
| `subStringCanAutoDetect` | `SubStringFormatter.CanAutoDetect` | `true` |
| `isNullSplitChar` | `NullFormatter.SplitChar` | one of `|`, `,`, `~` |
| `isNullCanAutoDetect` | `NullFormatter.CanAutoDetect` | `true` |
| `listSplitChar` | `ListFormatter.SplitChar` | one of `|`, `,`, `~` |
| `listCanAutoDetect` | `ListFormatter.CanAutoDetect` | `false` |
| `templates` | which set `TemplateFormatter.Register` is called with | `Standard`, `WithEmptyName`, `CaseInsensitive`, `Simple` |
| `variables` | which set of groups a `PersistentVariablesSource` is registered with | `Standard`, `Precedence`, `Shadowing` |
| `localization` | how the `ILocalizationProvider` is configured | `Fallback`, `ReturnName` |

`SplitChar` is validated by `Utilities.Validation.GetValidSplitCharOrThrow`, which accepts
only `|`, `,` and `~`, so those are the only values a case may ask for.

`TemplateFormatter` is not in `CreateDefaultSmartFormat`, so a case only has one when it
names a template set. The four sets are built by `TemplateFixture` in `Program.cs` and
mirrored name for name by `template_fixture` in the Rust runner — .NET fixes the
registry's comparer at construction and its `Dictionary.Add` throws on a duplicate, which
is why the case-insensitive set is the standard one minus `LAST`.

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
| `{"$ts": "1.01:01:01.0010000"}` | `TimeSpan`, parsed with the `"c"` round-trip format |
| `{"$f": "NaN" \| "Infinity" \| "-Infinity"}` | `double` |
| `{"$i32": "-255"}` | `int` (32-bit), for the cases pinning .NET's per-type integer width |

The `$ts` payload is `[-][d.]hh:mm:ss[.fffffff]`, whose seven fractional digits are
exactly .NET's 100 ns tick, so the wire form is lossless in both directions — `time_span`
in the Rust runner reads it into the `jiff::SignedDuration` a `Value::TimeSpan` holds, and
`TimeSpan.MinValue` round-trips.

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
an unnamed `|`-separated format, the `\uXXXX` sequences the parser reads past (the `uesc-*`
group), how a culture *name* resolves, the culture data, the M3 formatters — `list-*`,
`substr-*`, `isnull-*`, `ismatch-*`, `template-*` — and the M4 extensions: `time-*` and
`tsdefault-*` for the time formatter and for a `TimeSpan` through `DefaultFormatter`,
`loc-*` for localization, `var-*` for the persistent variables source, and `conddate-*` /
`condts-*` for the two conditions that were deferred until a clock could be pinned.
Numeric, date, plural and culture cases are generated combinatorially from a value or
culture list crossed with a specifier list, which is where most of the volume comes from.

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
exception the evaluator wraps. The M3 formatters carry their own `*-errtext-*` cases in
their own groups, for the same reason.

The `ismatch-*` patterns stay inside the subset `fancy-regex` and .NET read the same way;
the constructs the two engines disagree on are pinned by unit tests in `ismatch.rs`
instead, with one held here as a knowingly-skipped case
(`ismatch-dollar-before-final-newline`). See "IsMatch runs on fancy-regex" in `DESIGN.md`
for the full list.

Some cases exist only to pin .NET behavior the port deliberately does not match. They are
in the table like any other case, and the Rust runner names each of them in its `SKIPPED`
list with the reason; every entry under "Known divergences" in `DESIGN.md` points at one of
those ids or at a unit test.

One area is deliberately left out, because it falls outside the port's scope: custom
numeric, date and `TimeSpan` patterns. Two `tsdefault-*` cases pin what .NET does with one
anyway, and the Rust runner skips them with that reason.

Two shapes of localization case are left out for a different reason — the port does not
implement them yet, so a golden for either would be red on arrival. Both are pinned by
unit tests in `localization.rs` that name what has to change: a translation that formats a
number or a date *while the formatter options name the culture* (.NET assigns the culture
to `FormatDetails.Provider`, which the port cannot do until `FormattingInfo` grows a
`set_culture`), and a key that only matches after the format's own nested placeholders have
been rendered.

Three extensions read the **thread** culture instead of the `IFormatProvider` of the call,
which the port has nowhere to read: `ChooseFormatter` and `IsMatchFormatter` when they
stringify the value, and `TimeFormatter` when it writes a unit's number. The harness sets
the thread culture to the invariant one, so the machine that regenerates the table no
longer decides the answer — but a case whose *call* culture is something else and whose
value goes through one of those three would pin a divergence rather than agreement. Keep
these out:

- float or decimal values as `choose` options (`{0:choose(1.5|2.5):a|b}`).
- non-string values matched by `ismatch` whose `ToString()` reads culture data
  (`{0:ismatch(^1\\.5$):yes|no}` on `1.5`). Integers, bools and strings are safe.
- a negative `TimeSpan` through `{…:time:…}` under a culture whose negative sign is not a
  hyphen (`sv`, `fi`, `nb` in the generated set). The `time-full-*-neg` cases are called
  with the invariant culture and name their language in the options, which is exactly the
  shape that stays clear of this.

Date conditions (`{0:cond:Past|Today|Future}`) used to be on that list too, because
`ConditionalFormatter` reads a clock. They are in the table now: the clock is
`SystemTime.Now()`, not `DateTime.UtcNow`, and the harness pins it (see "The pinned clock"
above).

One more ordering rule, enforced by the harness rather than by review:
`ListFormatter.CollectionIndex` is a **static** in .NET, so a case whose list iteration
fails part-way leaves it set for the rest of the process and every later `{Index}` — under
any settings, through any formatter instance — reads the leaked value instead of `-1`. The
render loop therefore checks an `{Index}` canary after every case and fails the build
unless the only case that leaves it set is the last one in the table. Such cases go in
`CollectionIndexPoisoningCases`, which is called last.
