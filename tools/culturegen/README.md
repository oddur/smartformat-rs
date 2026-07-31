# Culture-data generator

This console app reads `CultureInfo.NumberFormat` and `CultureInfo.DateTimeFormat` out of a
real .NET runtime and prints them as a Rust source file. The Rust port formats numbers and
dates from that table, so for every culture listed here the port's output matches .NET by
construction — the data is not mapped from CLDR, it *is* .NET's data.

Regenerate from the repository root:

```sh
dotnet run --project tools/culturegen > crates/smartformat/src/fmt/culture/generated.rs
cargo fmt --all
```

The `cargo fmt` step is not optional: the generator emits each array on one line and
rustfmt decides how to wrap it, so the checked-in file is the formatted one and CI's
`cargo fmt --all --check` fails without it.

## The caveat that matters

.NET's culture data is ICU-backed. On Linux and macOS that means the ICU the machine
happens to carry; on Windows, the one shipped with the runtime. **It drifts.** Group
separators have changed from `U+00A0` to `U+202F`, month abbreviations have gained and lost
trailing dots, and default decimal digits have moved, all between ICU releases — with no
change on our side.

So `generated.rs` and `goldens/m1.json` are one artifact in two files. Regenerate both with
the same SDK on the same machine, in the same commit:

```sh
dotnet run --project tools/culturegen > crates/smartformat/src/fmt/culture/generated.rs
dotnet run --project tools/goldens > goldens/m1.json
cargo fmt --all
```

Regenerating one without the other produces a port whose culture data and whose expected
output come from different ICU versions, and the failures look like port bugs. The header
comment in `generated.rs` records the runtime and OS it came from so a surprising diff can
be traced back.

`InvariantGlobalization` is off in `culturegen.csproj`, because with it on every culture
resolves to the invariant one and the tool would silently emit 35 copies of it. It checks
at startup that `de-DE` really has a decimal comma and refuses to run if it does not.

## Cultures

```
"" en en-US en-GB de de-DE de-AT de-CH fr fr-FR es es-ES es-MX it nl pt pt-BR pt-PT
ru pl cs tr sv da fi nb uk is is-IS ja ko zh-Hans zh-CN ar ar-SA
```

The list lives at the top of `Program.cs`. Adding a name there and regenerating is the
whole cost of shipping another culture.

`fmt::culture::get` matches these names in full, case-insensitively, exactly like .NET's
`CultureInfo.GetCultureInfo`. There is no parent fallback: `"de-XX"` is `None`, not `"de"`,
because .NET would resolve it against the whole CLDR tree and we only have what is in the
table. The invariant culture (`""`) is answered from the hand-written `INVARIANT` in
`fmt/culture/mod.rs`; the table carries a generated copy of it and a unit test asserts the
two agree.

## What is emitted

Every field of `NumberFormat` and `DateTimeFormat` in `crates/smartformat/src/fmt/culture/mod.rs`,
each straight from the .NET property of the same name. Two of them exist only because a
real culture needed them:

- `number_negative_pattern` — .NET `NumberNegativePattern`. Every culture here is `1`
  (`-n`), but the port used to hard-code that, and a hard-coded "every culture" is how a
  port quietly stops matching.
- `month_genitive_names` / `abbreviated_month_genitive_names` / `use_genitive_month` —
  .NET `MonthGenitiveNames` and the `DateTimeFormatFlags.UseGenitiveMonth` flag.
  `ru`, `pl`, `cs`, `uk`, `fi` inflect the month next to a day number (`март` alone but
  `5 марта`), and `de`, `da`, `nb` do it in the abbreviated form only (`Mär` but
  `5 März`). Without these, every long date in those cultures is wrong.

Non-ASCII letters and digits stay literal so month and day names read as themselves;
everything else non-ASCII is escaped as `\u{...}`. That is deliberate. Real culture data is
full of characters no reviewer can tell apart by eye: `fr` groups with `U+202F` NARROW
NO-BREAK SPACE while `pt-PT` and `ru` use `U+00A0`, `sv` and `fi` negate with `U+2212`
MINUS SIGN rather than a hyphen, and `ar-SA` hides `U+061C` ARABIC LETTER MARK inside its
signs and `U+200F` inside its currency symbol. Escaped, they survive an editor, a
copy-paste and a "trim trailing whitespace" unchanged.

Four .NET properties are deliberately not carried:

- `PerMilleSymbol`, because no standard numeric specifier renders it.
- `NativeDigits` and `DigitSubstitution`, because .NET Core never substitutes native digits
  when formatting — `ar-SA` numbers come out with ASCII digits and Arabic-Indic separators.
- `ShortestDayNames`, because only the `ddd`/`dddd` tokens exist in the standard specifiers.
- The 13th month-name slot .NET pads its arrays with. The generator *checks* it is empty
  and fails if it is not, which is how a leap-month calendar would announce itself rather
  than losing a month.

## Known divergence: `ar-SA` dates

.NET's default calendar for `ar-SA` is `UmAlQuraCalendar`, so .NET renders 2024-03-05 as
`24 شعبان، 1445 بعد الهجرة`. The port renders Gregorian fields through those (Hijri) month
names and produces a different date. Numbers for `ar-SA` are unaffected and do match. The
divergence is pinned by
`fmt::date::tests::cultures::ar_sa_dates_diverge_because_its_calendar_is_not_gregorian`.
