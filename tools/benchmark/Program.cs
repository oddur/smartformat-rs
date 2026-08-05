// The .NET half of the cross-runtime benchmark. See tools/benchmark/README.md
// for how to run this and its Rust twin, and
// docs/explanation/dotnet-comparison.md for what the numbers turned out to be.
//
//     dotnet run --project tools/benchmark -c Release
//     dotnet run --project tools/benchmark -c Release -- --verify

using BenchmarkDotNet.Configs;
using BenchmarkDotNet.Running;

using SmartFormatBenchmark;

const string usage = """
    usage: dotnet run --project tools/benchmark -c Release [-- --verify]

      (no arguments)   run the BenchmarkDotNet suite
      --verify         print each shape's template and rendered bytes, and exit
    """;

if (args.Length == 1 && args[0] is "--help" or "-h")
{
    Console.Error.WriteLine(usage);
    return 0;
}

if (args.Length == 1 && args[0] == "--verify")
{
    // The rendered bytes each shape must produce, so they can be diffed
    // against the Rust twin's. `Setup` already asserts them; printing them
    // makes the assertion readable rather than only enforced.
    var benchmarks = new RenderBenchmarks();
    benchmarks.Setup();
    foreach (var (name, template, _) in RenderBenchmarks.Shapes)
        Console.WriteLine($"{name}\t{template}\t{benchmarks.Render(name)}");
    return 0;
}

if (args.Length != 0)
{
    Console.Error.WriteLine($"unknown argument '{args[0]}'\n{usage}");
    return 2;
}

// The default job's full warmup and iteration counts. A `ShortRun` finishes in
// a couple of minutes but gives some rows a confidence interval wider than the
// mean, which is no basis for a published comparison.
//
// The artifacts go to a temporary directory, not the repository. This suite
// generates, restores and compiles a project per benchmark, which is a few
// gigabytes, and it once filled the machine's disk mid-run. Keeping it out of
// the tree means a failed run leaves nothing behind to commit or clean up.
var config = DefaultConfig.Instance
    .WithArtifactsPath(Path.Combine(Path.GetTempPath(), "smartformat-benchmark"));

BenchmarkRunner.Run<RenderBenchmarks>(config);
return 0;
