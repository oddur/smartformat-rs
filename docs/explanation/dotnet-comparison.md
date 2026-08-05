# How the port compares to SmartFormat.NET

The port renders every measured shape faster than SmartFormat.NET, by between
1.6 and 12.6 times. That headline needs its caveats, and they are further down
this page rather than in a footnote, because a benchmark between two runtimes
is easy to write and hard to write fairly.

The two halves render the same templates against the same data and produce the
same bytes. That last part is enforced: `crates/smartformat/tests/bench_shapes.rs`
asserts the Rust side produces exactly what the C# side prints in its `--verify`
mode. A benchmark comparing two implementations that disagree measures nothing.

## The numbers

Apple M5 Max, macOS 26.4.1, .NET SDK 10.0.201 with BenchmarkDotNet 0.15.8,
rustc 1.97.1 with criterion 0.5. Both sides use the invariant culture except
the `_de` row. Means, in nanoseconds per render.

| Shape | SmartFormat.NET | This port | Ratio |
|---|---:|---:|---:|
| `render_selectors` | 310.7 | 85.6 | 3.6× |
| `render_number_spec` | 798.6 | 95.2 | 8.4× |
| `render_number_spec_de` | 834.5 | 116.1 | 7.2× |
| `render_plural` | 271.1 | 165.1 | 1.6× |
| `render_choose` | 219.4 | 88.4 | 2.5× |
| `render_list` | 1588.3 | 126.2 | 12.6× |
| `render_nested` | 556.5 | 292.7 | 1.9× |
| `parse_simple` | 613.9 | 273.8 | 2.2× |
| `format_oneshot` | 659.3 | 372.3 | 1.8× |

Every row but the last two renders an already-parsed template. `parse_simple`
parses only, and `format_oneshot` parses and renders in one call.

.NET's arguments are a `Dictionary<string, object>`, which is the closest
analogue to the `Value` tree the port renders against. Most .NET callers pass a
plain object instead and resolve through `ReflectionSource`, so three rows were
also measured that way: `render_selectors` 352.5 ns, `render_nested` 593.3 ns,
`format_oneshot` 703.1 ns. Reflection costs .NET 7% to 13% over dictionaries
here, less than its reputation suggests, because SmartFormat caches what it
looks up.

## What is not comparable

**The port is handed a value tree it did not pay to build.** `Value::Map` and
`Value::List` have to be constructed by the caller, and that construction is
outside the measurement. The .NET dictionary rows have the same shape, since
the dictionary is built in `[GlobalSetup]`, so those rows are close to fair.
The reflection rows are not: .NET is reading fields off a live object, which is
work the port pushes onto the caller. If your data starts as structs, budget
for `#[derive(ToSmartValue)]` building the tree on every render.

**The two number rows are noisy on the .NET side.** `render_number_spec` has a
standard deviation of 280 ns against a mean of 798, and its `_de` twin 319
against 834. The allocation columns show why: those rows trigger Gen1 and Gen2
collections, so a garbage collection lands inside some iterations and not
others. The ratio for those two rows is real in direction and soft in
magnitude. Read them as "several times faster", not as 8.4.

**Allocation behaviour differs in kind, not degree.** .NET allocates between
384 B and 2104 B per render on these shapes and pays for it later, in a
collector this benchmark only partly accounts for. The port's equivalent paths
allocate the output string and, on most of these shapes, little else. That is
an architectural difference, not a tuning one, and it is most of why
`render_list` shows the widest gap: .NET allocates 2104 B to render a
three-item list.

**Measurement tooling differs.** BenchmarkDotNet and criterion both warm up and
both report robust statistics, but they are not the same instrument, and the
two suites ran in separate processes minutes apart. Treat differences under
roughly 20% as noise between tools rather than signal about code.

**These are microbenchmarks of one call.** They say nothing about throughput
under concurrency, memory pressure in a long-running service, or startup, and
nothing about an application where formatting is a fraction of the work. A
template render is rarely the bottleneck; if it is yours, measure your own
templates rather than trusting this table.

**One machine, one day.** Apple silicon, a single run of each suite. Numbers on
a different CPU or a different .NET runtime will differ, possibly a lot.

## Where the difference comes from

Three things, roughly in order of contribution.

Parsed templates carry their split pieces, so a formatter that cuts a format on
`|` does that cutting once for the life of the parsed template rather than once
per render. This is why the split-driven shapes (`list`, `choose`, `plural`)
show the widest gaps, and it is the trade described in
[the performance model](performance.md): a parsed `Format` is larger, and a
caller who parses and discards gains nothing.

Numbers format without allocating. The common specifiers write digits into the
output string through a stack buffer, and the exact-decimal machinery only runs
for values that need it.

Values are not boxed. .NET's sources hand back `object`, so an integer selector
result is a heap allocation before it is ever formatted, while `Value` carries
its integers inline.

## Reproducing this

`tools/benchmark/README.md` has the commands for both halves. The C# suite
writes its artifacts to a temporary directory rather than the repository: it
generates and compiles a project per benchmark, which is several gigabytes, and
it filled this machine's disk the first time it ran.
