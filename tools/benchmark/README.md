# Cross-runtime benchmark

The .NET half of a benchmark that compares this port against the real
SmartFormat.NET on identical templates and data. The Rust half is
`crates/smartformat/benches/render.rs`. The results, with what they do and do
not mean, are in [docs/explanation/dotnet-comparison.md](../../docs/explanation/dotnet-comparison.md).

## Running it

Both halves, from the repository root:

```console
$ dotnet run --project tools/benchmark -c Release
$ cargo bench -p smartformat --bench render
```

The C# suite takes upwards of twenty minutes: BenchmarkDotNet generates,
restores and compiles a separate project per benchmark. Those artifacts go to
`$TMPDIR/smartformat-benchmark`, not into the repository, because they run to
several gigabytes and once filled this machine's disk mid-run. Delete that
directory when you are done.

## Checking the two halves agree

```console
$ dotnet run --project tools/benchmark -c Release -- --verify
```

prints each shape's template and the bytes it renders. `cargo test --test
bench_shapes` asserts the Rust side produces exactly those bytes. Run both
after changing a shape: a benchmark comparing two implementations that disagree
measures nothing.

## Adding a shape

Add it in three places, or the comparison quietly stops being one:

1. `RenderBenchmarks.Shapes` here, with the template and the bytes it must
   render.
2. A `[Benchmark]` method here that renders it.
3. The matching criterion bench in `crates/smartformat/benches/render.rs`,
   under the same name.

Then extend `crates/smartformat/tests/bench_shapes.rs` so the new shape's bytes
are pinned on the Rust side too.

Arguments come in two forms. The `(dict)` rows pass a
`Dictionary<string, object>`, which is what the port's `Value::Map` corresponds
to, and those are the rows the comparison table uses. The
`(POCO, reflection)` rows pass a plain object so `ReflectionSource` resolves the
selectors, which is what most .NET callers actually write; the port has no
equivalent, so those rows stand alone as context rather than as a comparison.
