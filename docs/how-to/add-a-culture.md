# Add a culture the crate does not ship

Ship a locale that is not in the 35 the crate carries, by regenerating the culture table from a real .NET runtime.

You cannot hand-write one. Read [Why a culture cannot be hand-written](#why-a-culture-cannot-be-hand-written) first if you are tempted; the rest of this guide is the four steps.

## 0. Check whether you actually need it

`fmt::culture::get` matches names in full and case-insensitively, exactly like .NET's `CultureInfo.GetCultureInfo`, with **no parent fallback**. `"de-XX"` is `None`, not `"de"`.

```rust
use smartformat::fmt::culture;

assert!(culture::get("de-DE").is_some());
assert!(culture::get("DE-de").is_some());   // case-insensitive
assert!(culture::get("de-XX").is_none());   // no fallback to "de"
```

So a template that needs `de-AT` needs `de-AT` in the table. If yours needs `nl-BE` and `nl` is close enough for the numbers and dates you render, use `nl` and skip this guide. If it is not close enough, the difference is exactly what the table exists to carry, and you need the real thing.

Two names resolve to something other than what they look like, as they do in .NET. A name with an alternate sort order after an `_` drops the sort order and the region: `en_US` is the language `en`, not `en-US`. Check what you got before you conclude a culture is missing.

## 1. Add the name to the generator

The list lives at the top of `tools/culturegen/Program.cs`, in a `string[] requested` array:

```csharp
string[] requested =
[
    "", "en", "en-US", "en-GB", "de", "de-DE", "de-AT", "de-CH", "fr", "fr-FR", "es", "es-ES",
    // ...
];
```

Add your name to it. Keep the same list in the table at the bottom of `tools/culturegen/README.md`; the two are meant to agree, and nothing checks it for you.

Order does not matter. The generator sorts by ASCII-lowercase name, because the Rust lookup binary-searches the emitted array.

## 2. Regenerate the culture table and the goldens together

`tools/culturegen/README.md` has the commands. Run both of them, in one commit, with the same SDK on the same machine.

The reason is in that README and it is not optional: .NET's culture data is ICU-backed, so on Linux and macOS it comes from whatever ICU the machine carries. Group separators have moved from `U+00A0` to `U+202F`, month abbreviations have gained and lost trailing dots, and default decimal digits have changed, all between ICU releases with no change to any of this code. `crates/smartformat/src/fmt/culture/generated.rs` and `goldens/m1.json` are one artifact in two files. Regenerate one without the other and you get a port whose culture data and whose expected output come from different ICU versions; the failures look like port bugs and are not.

Run `cargo fmt --all` afterwards. The generator emits each array on one line and rustfmt decides how to wrap it, so the checked-in file is the formatted one and CI's `--check` fails without this step.

## 3. Let the tool check what it can

The generator refuses to produce data it knows is meaningless, so several classes of mistake fail the run rather than reaching the table:

- **Invariant globalization.** It asserts at startup that `de-DE` really has a decimal comma. With `InvariantGlobalization` on, every culture resolves to the invariant one and the tool would emit 35 identical copies; instead it stops.
- **A leap-month calendar.** .NET pads its month-name arrays with a 13th slot. The generator checks that slot is empty and fails if it is not, so a calendar with thirteen months announces itself instead of quietly losing one.
- **Non-visual characters.** Everything non-ASCII except letters and digits is escaped as `\u{...}`. That is deliberate: real culture data is full of characters no reviewer can tell apart by eye, and escaped ones survive an editor, a copy-paste and a "trim trailing whitespace" unchanged.
- **Provenance.** The header of `generated.rs` records the runtime and OS that produced it, so a surprising diff can be traced back to a machine.

What it does not check is whether your culture is one .NET renders through a non-Gregorian calendar. `ar-SA` is the one already in the table: .NET's default calendar for it is `UmAlQuraCalendar`, so .NET renders 2024-03-05 as `24 شعبان، 1445 بعد الهجرة`, and this crate renders Gregorian fields through those Hijri month names and produces a different date. Numbers are unaffected. If your new culture has the same property, its dates will diverge the same way, silently. Check `CultureInfo.GetCultureInfo(name).Calendar` before you trust the date output.

## 4. Confirm it landed

Add an assertion where the rest of the culture tests live, and check the specifiers whose output is pure culture data: `N`, `C` in both signs, `P1`, and the `d` / `D` / `t` / `T` / `f` date patterns. Those are exactly what no reviewer can verify by eye.

```rust
use smartformat::fmt::culture;
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::Float(-1234.5)]);

let de_at = culture::get("de-AT").expect("shipped");
assert_eq!(de_at.name, "de-AT");
assert_eq!(smart.format_with_culture("{0:N2}", &args, de_at).unwrap(), "-1.234,50");
```

The golden harness generates these combinatorially: every culture in the table crossed with that specifier list. Regenerating in step 2 therefore adds the cases for your culture automatically, and `cargo test -p smartformat` runs them. A red case there means your ICU and the .NET you generated with disagree, which is the answer you wanted the test to give you.

## Why a culture cannot be hand-written

`CultureData` is not a description of a locale. It is a verbatim copy of .NET's `CultureInfo.NumberFormat` and `CultureInfo.DateTimeFormat`, field for field, read out of a running .NET process. The whole compatibility claim rests on that: for a listed culture the port matches .NET **by construction**, because it is formatting from .NET's own numbers.

Typing the fields by hand replaces "by construction" with "by inspection", and inspection fails on this data. Three examples out of the table as it stands:

- `fr` groups digits with `U+202F` NARROW NO-BREAK SPACE while `pt-PT` and `ru` use `U+00A0`. On screen they are the same blank.
- `sv` and `fi` negate with `U+2212` MINUS SIGN, not a hyphen.
- `ar-SA` hides `U+061C` ARABIC LETTER MARK inside its signs and `U+200F` inside its currency symbol.

And the shape of the data is not obvious either. `ru`, `pl`, `cs`, `uk` and `fi` inflect the month name next to a day number (`март` alone, `5 марта`), and `de`, `da` and `nb` do it in the abbreviated form only. There are 17 currency-pattern arms. Getting a locale right means getting all of that right, and getting it wrong means a port that is subtly, permanently different from the library it claims to match.

Adding a name to `Program.cs` and rerunning the generator is the whole cost, which is why that is the supported path.
