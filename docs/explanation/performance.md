# The performance model, and its evidence

The crate is built to parse once and render many times, and the render path is where the
work went. On the machine the benchmarks were last run on, rendering a pre-parsed
`Hello {Name}, you have {Count} items` takes about 81 ns and parsing the same template takes
about 271 ns. Both figures come from one machine on one day, and the last section is about
what such figures do and do not establish.

## The shape the API is built for

`SmartFormatter::parse` returns a `Format`, and `format_parsed` renders it. `format` is the
convenience that does both and throws the tree away. Parsing is the expensive half, so a
template that renders per request wants to be parsed once, at startup, and kept.

A `Format` is immutable in use and shareable across threads, and so is a `SmartFormatter`,
which holds no mutable per-call state. One formatter behind an `Arc`, a map of parsed
templates beside it, and any number of threads rendering is the intended arrangement.
[How a render happens](architecture.md) covers why the per-call state lives on the call
rather than in statics, which is what makes that arrangement safe.

There is no template cache inside the crate, deliberately. A cache needs a key policy and an
eviction policy, and both belong to the host: a fixed set of templates loaded at startup
wants a `HashMap` and no eviction, while templates arriving from user data want a bound and a
strategy for exceeding it. The one place the crate does cache parsed formats is
`LocalizationFormatter`, because translations only become strings during a render and no
caller is in a position to key them. [Test your templates](../how-to/test-your-templates.md)
builds that map as a side effect of its startup parse check, which is the shape this section
argues for.

## What the split store buys, and what it trades

Six formatters read their parts by splitting the parsed format: `plural`, `cond`, `choose`,
`isnull`, `ismatch` and `list`. Splitting is not cheap. Each piece allocates its own raw
text, its own item vector, a fresh literal for each end the cut passes through, and a clone of
every placeholder in between. Every one of those formatters used to do that again on every
render, which for a cached template was over half the work.

A `Format` now remembers the pieces it was cut into, keyed by the separator and the limit, in
two slots. Two rather than one because auto-detection hands the same format to every
auto-detecting formatter in turn and they do not split it alike: `list` stops after four
separators, where the other five split without a limit. With a single slot the first
formatter to touch an unnamed placeholder filled it with a shape none of the others ever
asked for, and every render after that cut the format afresh. The gap showed up exactly where
the store was meant to help: `{Count:one item|{} items}` cost 347 ns against 160 ns for its
named twin `{Count:plural:one item|{} items}`. With two slots the auto-detected form costs
about what the named one does.

A third shape, which takes a formatter registered with a separator of its own, is cut and
thrown away, exactly as every split was before the store existed. And a format holding no
separator is *lent* rather than copied: .NET splits such a format into itself, all a
formatter does with a one-piece split is count it and decline, so `{0:N2}` is offered to
three auto-detecting formatters per render at no allocation at all.

The trades are worth naming.

**Memory, paid by the caller who parses and discards.** The slots hang off a `OnceLock<Box<…>>`
allocated the first time a format is split, so a `Format` stays 80 bytes and a `FormatItem`
256, which matters because a `Format` is embedded in every item of every format. Inline slots
were measured and rejected: they grew `Format` to 96 bytes and `FormatItem` to 272, cost
`format_oneshot` 2.8% and `parse_simple` 5.3%, and bought 2 ns on one bench. Behind the
pointer, one-shot formatting comes out flat, and a format that is split holds its pieces for
as long as it lives.

**API rigidity.** The pieces are cut from the parts of the tree as they were, so a caller who
changed an item between two renders would render the format it used to be. `items`, `raw`,
`start` and `end` became accessors, `Format::new` became the way to build a changed tree, and
a clone starts with empty slots. A doc comment used to ask for that; the type enforces it now.

**Nothing else.** No answer moves. The pieces are cut eagerly as they always were, a piece
that cannot be cut is still an `Err` of its own, and .NET's per-piece laziness, where
`{0:choose(1|2|3):a|b|\u12}` fails only for the argument 3, is where it was.

## Allocation on the render path

The selector path allocates exactly once per render: the output string, sized from the
template's own length with a floor of 16 bytes. Everything under it was worked back to
borrowing:

- An integer with no format specifier, the commonest placeholder after a plain string,
  renders into a stack buffer: the culture's negative sign and the decimal digits, and nothing
  else. A specifier of any kind falls back to the full number formatter.
- Strings, booleans and nulls are written from the value that already exists.
- A registered source lends what it stores. `PersistentVariablesSource` hands out a borrow of
  its group and the leaf inside it, because a registered source is owned by the formatter
  doing the formatting and therefore outlives the call. `GlobalVariablesSource` still copies,
  because its groups sit behind a lock and a read guard cannot outlive the call.
- Numbers with a specifier no longer run big-integer machinery for ordinary values. A
  mantissa's trailing zero bits cancel against a negative binary exponent, and once cancelled
  the exact decimal expansion of essentially every value a template formats fits in 128 bits.
  Digits live in an inline buffer, grouping walks the culture's group sizes without
  materializing them, and the wide expansions keep a big-integer path over a fixed limb array.
- `IsMatchFormatter` compiles each pattern once and keeps it, failures included, where .NET
  builds a `new Regex(…)` per call.
- The parser indexes the input in place. An all-ASCII template, which nearly every template
  is, needs no character tables at all; only a template with a non-ASCII character pays for
  them, and even then the UTF-16 offsets are tabulated on the first error that reports one.

## The benchmarks

`crates/smartformat/benches/render.rs` is a criterion suite, run with
`cargo bench -p smartformat --bench render`. Each render bench formats a pre-parsed template,
which is the parse-once shape the API is built for, against one small map holding a name, a
count, a gender flag and a three-item list. The number benches format a single float instead,
and `format_oneshot` is the one bench that measures parse plus render together.

| Benchmark | What it measures | Last measured |
|---|---|---|
| `render_selectors` | `Hello {Name}, you have {Count} items`, pre-parsed | ~81 ns |
| `render_choose` | `{Gender:choose(m\|f):his\|her}` | ~82 ns |
| `render_number_spec` | `{0:N2}` over a float, invariant culture | ~91 ns |
| `render_list` | `{Items:list:{}\|, \|, and }` over three strings | ~121 ns |
| `render_plural` | `{Count:plural:one item\|{} items}` | ~157 ns |
| `parse_simple` | parsing `Hello {Name}, you have {Count} items` | ~271 ns |
| `format_oneshot` | the same template parsed *and* rendered | ~364 ns |

The file carries six more: `render_number_spec_de` (the same number under a culture with a
different separator), `render_variables` (a `{group.variable}` answered by a registered
source), `parse_nested`, and three `*_autodetect` twins of the plural, conditional and list
benches. The twins earn their place: they render the same formats with the formatter name
left out, which is the case a single-slot split store silently stopped serving, so a store
that regresses shows up as a doubling rather than as silence.

Against the first measurement taken when the benches landed, every one of these is between
roughly two and seven times faster. The number path is the extreme, from about 610 ns to
about 91 ns, and the pre-parsed selector render the mildest, from about 139 ns to about
81 ns. Those two figures come from different sessions on different days, so treat the factor
as a magnitude rather than a measurement.

## What the numbers do not say

**They are one machine's.** A criterion figure is a property of a CPU, a compiler version and
an otherwise idle machine. The absolute nanoseconds do not transfer; the ratios within a
single run do, which is why every performance change in the history was measured back to back
against its own baseline in one session.

**They say nothing about SmartFormat.NET.** No cross-runtime comparison has been made and
none is claimed. The port's promise is that the bytes match, not that it is faster than the
library it copies. The benchmarks compare the crate against earlier versions of itself.

**They say nothing about other Rust template engines either.** SmartFormat's syntax and .NET
semantics are the requirement here; a comparison against an engine with different semantics
would measure the semantics, not the implementation.

**They cover short templates over small values.** Nothing in the file measures a
four-kilobyte template with two hundred placeholders, a list of ten thousand items, a deeply
nested format, or a template full of non-ASCII text, which is exactly where the parser's ASCII
fast path stops applying. Those shapes are unmeasured, not fast.

**They count time, not allocations.** The allocation claims above come from reading the code
and from the time the removals bought, not from an allocation counter in the suite.

**Nothing measures them automatically.** CI runs formatting, clippy, the test suite, a docs
build and an MSRV check. No benchmark and no regression gate runs there, so a performance
regression is invisible until someone measures. The benches are the tool for that, and the
practice is to run them back to back against a stashed baseline in the same session.

One thing is checked automatically, and it is the one that matters most: the 2,764 goldens run
in CI on every change. Every optimization in this history left `goldens/m1.json` untouched,
and a faster path that changed an answer is not a trade this project makes. See
[how compatibility is verified](how-compatibility-is-verified.md).

These figures measure this port against its own history. For the same shapes measured against
SmartFormat.NET itself, see [how the port compares to SmartFormat.NET](dotnet-comparison.md).
