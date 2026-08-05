# Why byte-identical output is the goal

The port has one hard constraint: a template written for SmartFormat.NET renders the same
bytes here, not merely similar text. Every other design question in the crate is settled
against that constraint, and where the two sides cannot agree, the disagreement is written
down rather than smoothed over.

## Why "close enough" is the wrong target

Templates outlive the code that renders them. A .NET service that has accumulated a few
thousand message templates over a decade has no test for most of them; the templates
themselves are the specification, and the only definition of "correct" anyone can appeal to
is what the old renderer produced. A port that gets 99% of them right moves the remaining
1% into production, where the failures are quiet and unevenly distributed.

They are quiet because the differences that survive a careless port are the ones nobody
looks at. Which space character a French locale groups digits with (U+202F, where Russian
uses U+00A0). Whether Swedish negates with a hyphen or U+2212. Whether a Polish long date
inflects the month next to the day number. Which of .NET's seventeen negative-currency
patterns a locale lands on. Whether `{0:F}` on 2.675 rounds to `2.67` or `2.68`. No reviewer
catches those by eye, no user reports them precisely, and a diff against the old output is
the only instrument that finds them at all. Byte equality is what makes that diff usable: a
migration either produces an empty diff or produces a list of exact places to look.

They are unevenly distributed because the hard cases cluster. Pluralization, currency,
genitive months and error text are exactly where a "reasonable approximation" and .NET part
company, and exactly where the consequences are visible to a user in a language the team
does not read.

Byte equality is also the only compatibility claim that can be tested cheaply. "Renders
correctly" needs a human judgment per case. "Renders the same bytes as .NET" needs a table
of cases and a string comparison, which is what
[how compatibility is verified](how-compatibility-is-verified.md) describes.

## What the goal rules out

The largest consequence is that the port refuses work it cannot do exactly, instead of
approximating it. .NET *custom* numeric, date and `TimeSpan` patterns are the main example:
`{0:#,##0.00}`, `{0:yyyy-MM-dd}` and `{0:hh\:mm}` all render in .NET and are an
`UnsupportedSpec` error here. Reimplementing them means reimplementing the whole custom
pattern language, section separators, quoting rules and rounding included, and a
reimplementation that is 95% right is worse than nothing: the 5% renders plausible text
that silently differs. The standard specifiers (`B C D E F G N P R X` and the standard date
and `TimeSpan` patterns) are ported in full, so a template that uses `N2` where it could
have used `#,##0.00` works.

The same reflex applies elsewhere:

- A culture outside the generated list is `Error::UnknownCulture`, never a guess. .NET
  would resolve `de-XX` up the CLDR tree; the port has only what it read out of .NET, so it
  says so.
- Default formatting of a list or a map is an error. .NET falls back to `object.ToString()`
  and writes `System.Object[]`, which is never what a template author meant, so this is one
  of the few places the port refuses something .NET renders.
- Five .NET regex constructs fail to compile here rather than matching different text, and
  the `RegexOptions` members with no equivalent fail the call rather than being ignored. The
  constructs that compile on both engines and *mean* different things cannot be detected at
  all, so each of those is pinned by a test instead.

The cost of that reflex is a port that is less forgiving than the library it copies. The
benefit is that a template the port cannot handle produces an error with a position in it,
at the moment it is first rendered, instead of a subtly wrong string a year later.

## What the goal costs

**A divergence ledger.** [DESIGN.md](../../DESIGN.md) carries thirty-five entries under
"Known divergences", seven deliberate policy differences, six reproduced .NET quirks and a
section for behavior pinned to version 3.6.1. Every entry names the golden case or the unit
test that holds the other side's answer. The ledger is maintained by hand and updated in the same
change that moves a behavior, which is a standing tax on every change to rendering.

**Generated culture data.** The 35 entries in `fmt::culture::generated` (the invariant
culture included) were read out of a running .NET by `tools/culturegen` and printed as Rust
source. They are not mapped from CLDR, because mapped data would be close rather than
identical. The price is that the data is a snapshot: .NET's culture data is ICU-backed, and
ICU drifts. Group separators have moved from U+00A0 to U+202F, month abbreviations have
gained and lost trailing dots, and default decimal digit counts have changed between ICU
releases with no change on either side of the port. So `generated.rs` and `goldens/m1.json`
are one artifact in two files, regenerated together, on one machine, in one commit.

**A .NET dependency for test data.** The expected outputs come from the real library, so
regenerating them needs a .NET SDK and a network fetch of SmartFormat.NET 3.6.1. Both
artifacts are checked in, so building, testing and using the crate needs no .NET at all;
only changing the test corpus does.

**Bugs reproduced on purpose.** SmartFormat.NET's parser reads six characters for `\uXXXX`
without checking that four of them are hex digits, then advances by one, so those characters
are read again as template text and the tree can hold a literal whose end is before its
start. A template out there may depend on the result, so the port reproduces it, and the
shapes that forces (a split that can fail at two different moments, per-piece laziness, an
explicit separator limit for the list formatter) are load-bearing rather than accidental.
They are listed under "Reproduced .NET quirks" so nobody tidies them away.

**A pinned upstream version.** The goldens are generated with 3.6.1, and where upstream has
since changed behavior the port follows 3.6.1, not `main`. Compatibility is with a version,
not with a moving target.

## Where it stops: UTF-8 against UTF-16

The one boundary that no amount of care removes is the string model. A .NET `string` is a
sequence of UTF-16 code units and may hold an unpaired surrogate; a Rust `String` is
well-formed UTF-8 and cannot. Three behaviors sit on that fault line.

**Lone surrogates.** `\uD83D` on its own stays a code unit in .NET's result string. Here it
becomes U+FFFD, because nothing else can hold it. An escaped surrogate *pair* joins into its
character on both sides. The golden harness cannot see this one: its JSON writer transcodes
.NET's lone surrogate to U+FFFD on the way out, which is the character rendered here, so the
cases compare equal. The difference is real all the same, which is why the ledger carries it.

**`substr` counting.** The start index and length count UTF-16 code units, deliberately,
because .NET's `String.Substring` does: `{0:substr(2)}` over `"😀abc"` is `abc` on both
sides and `{0:substr(0,5)}` is the whole string. A cut can therefore land inside a surrogate
pair. A single orphaned half is still byte-identical, since .NET's UTF-8 encoding of a lone
surrogate is the same three bytes as U+FFFD. Two complementary halves written next to each
other are not: .NET still holds each half as a code unit and the pair re-forms into the
emoji, where each half here was replaced the moment it was cut. That case is the one skipped
golden in the group.

**Regex element counting.** `ismatch` runs on fancy-regex, which matches over Unicode
scalars where .NET matches over code units, so `^.$` against `"😀"` is "no" in .NET and
"yes" here, and `^..$` is the other way round. This is deliberately *not* aligned with
`substr`: each extension follows the .NET API it wraps, and the two .NET APIs disagree with
each other.

A fourth, smaller consequence: formatting-error positions count UTF-8 bytes where .NET
counts UTF-16 code units, so the two agree for ASCII templates and drift for others. Parse
errors do not diverge, because the caret line printed under the template has to line up with
the template above it.

## What byte-identity cannot mean

Some .NET behavior has no fixed answer to match. Three extensions read the *thread* culture
rather than the format provider passed to the call: `ChooseFormatter` and `IsMatchFormatter`
when they stringify a value, and `TimeFormatter` when it writes a unit's number. What .NET
prints there depends on the machine it runs on, so there is nothing stable to be identical
to. The port reads the culture of the call instead, which is the same answer whenever the
two agree, and the test corpus is kept clear of the cases where they do not. `SmartSettings`
takes "now" as an explicit setting for the same reason: .NET reads a process-wide mutable
clock, and a per-call setting is the only version of that which a test can pin.

These are listed as deliberate policy differences rather than divergences. They are the
places where reproducing .NET exactly would mean reproducing its dependence on ambient state,
which is not a property worth porting.

## Where the details live

[DESIGN.md](../../DESIGN.md) is the ledger: every known divergence, every deliberate policy
difference, every reproduced quirk, each with the golden case or test that pins it. This page
explains why the ledger exists; the ledger itself is the authority on what is in it.

- [How compatibility is verified](how-compatibility-is-verified.md): the golden-file method
  and its limits.
- [Run your .NET templates from Rust](../how-to/run-dotnet-templates.md): the practical
  migration path, including how to validate an existing corpus.
- [Format specifiers](../reference/format-specifiers.md) and
  [cultures](../reference/cultures.md): what is supported, in a table.
