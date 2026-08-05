using System.Globalization;

using BenchmarkDotNet.Attributes;

using SmartFormat;
using SmartFormat.Core.Parsing;

namespace SmartFormatBenchmark;

/// <summary>
/// The .NET half of the cross-runtime comparison. Every benchmark here has a
/// twin in <c>crates/smartformat/benches/render.rs</c> that renders the same
/// template string over the same values and produces the same bytes.
/// </summary>
/// <remarks>
/// <para>
/// Three things keep the halves comparable, and all three are easy to get
/// wrong:
/// </para>
/// <para>
/// <b>Parsing.</b> SmartFormat.NET 3.6.1 has no cache of parsed formats. Its
/// only caches are <c>ReflectionSource.TypeCache</c>, the read-only-dictionary
/// type cache in <c>DictionarySource</c>, and
/// <c>LocalizationFormatter.LocalizedFormatCache</c>, none of which is a
/// template cache. So <c>SmartFormatter.Format(provider, string, args)</c>
/// reparses on every call, exactly as the port's <c>format</c> does, and
/// <c>Parser.ParseFormat</c> plus <c>Format(provider, Format, args)</c> is the
/// parse-once shape, exactly as the port's <c>parse</c> plus
/// <c>format_parsed</c> is. The <c>Render*</c> rows use pre-parsed formats on
/// both sides; the <c>FormatOneshot*</c> rows reparse on both sides.
/// </para>
/// <para>
/// <b>Arguments.</b> The port renders a value tree with no reflection in it, so
/// the dictionary rows are the like-for-like comparison: a
/// <see cref="Dictionary{TKey,TValue}"/> reaches
/// <c>DictionarySource</c>, which is a lookup, and the port's
/// <c>Value::Map</c> is a lookup too. The <c>*Poco</c> rows format a plain
/// class through <c>ReflectionSource</c> instead, because that is what a .NET
/// caller usually writes. They are a separate measurement, not the headline.
/// </para>
/// <para>
/// <b>Culture.</b> Everything runs under the invariant culture except
/// <see cref="RenderNumberSpecDe"/>, which is the row about a culture.
/// <c>ChooseFormatter</c> reads the <i>thread</i> culture when it stringifies a
/// value rather than the provider of the call, so the thread culture is pinned
/// in <see cref="Setup"/> as well.
/// </para>
/// </remarks>
[MemoryDiagnoser]
public class RenderBenchmarks
{
    private const string SelectorsTemplate = "Hello {Name}, you have {Count} items";
    private const string NumberSpecTemplate = "{0:N2}";
    private const string PluralTemplate = "{Count:plural:one item|{} items}";
    private const string ChooseTemplate = "{Gender:choose(m|f):his|her}";
    private const string ListTemplate = "{Items:list:{}|, |, and }";

    private const string NestedTemplate =
        "{Name} has {Count:plural:one item|{} items} in {Gender:choose(m|f):his|her} cart";

    /// <summary>
    /// Every shape's template and the bytes it must render to. The Rust twin
    /// asserts the same table in
    /// <c>crates/smartformat/tests/bench_shapes.rs</c>, so a shape that stops
    /// measuring the same work on both sides fails rather than reports.
    /// </summary>
    public static readonly (string Name, string Template, string Expected)[] Shapes =
    [
        ("render_selectors", SelectorsTemplate, "Hello Alice, you have 3 items"),
        ("render_number_spec", NumberSpecTemplate, "1,234.50"),
        ("render_number_spec_de", NumberSpecTemplate, "1.234,50"),
        ("render_plural", PluralTemplate, "3 items"),
        ("render_choose", ChooseTemplate, "her"),
        ("render_list", ListTemplate, "sword, shield, and potion"),
        ("render_nested", NestedTemplate, "Alice has 3 items in her cart"),
    ];

    private readonly CultureInfo _invariant = CultureInfo.InvariantCulture;
    private readonly CultureInfo _german = CultureInfo.GetCultureInfo("de-DE");

    private SmartFormatter _smart = null!;

    // The value tree the port is handed, expressed the way the golden harness
    // maps JSON to CLR: an object becomes Dictionary<string, object>, an array
    // becomes object[], an integer becomes long, a fraction becomes double.
    private object[] _dictArgs = null!;
    private object[] _pocoArgs = null!;
    private object[] _floatArgs = null!;

    private Format _selectors = null!;
    private Format _numberSpec = null!;
    private Format _plural = null!;
    private Format _choose = null!;
    private Format _list = null!;
    private Format _nested = null!;

    [GlobalSetup]
    public void Setup()
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        CultureInfo.CurrentUICulture = CultureInfo.InvariantCulture;

        _smart = Smart.CreateDefaultSmartFormat();

        _dictArgs =
        [
            new Dictionary<string, object>
            {
                ["Name"] = "Alice",
                ["Count"] = 3L,
                ["Gender"] = "f",
                ["Items"] = new object[] { "sword", "shield", "potion" },
            },
        ];
        _pocoArgs = [new Person()];
        _floatArgs = [1234.5];

        _selectors = _smart.Parser.ParseFormat(SelectorsTemplate);
        _numberSpec = _smart.Parser.ParseFormat(NumberSpecTemplate);
        _plural = _smart.Parser.ParseFormat(PluralTemplate);
        _choose = _smart.Parser.ParseFormat(ChooseTemplate);
        _list = _smart.Parser.ParseFormat(ListTemplate);
        _nested = _smart.Parser.ParseFormat(NestedTemplate);

        // A benchmark that renders the wrong thing measures the wrong thing.
        // The de-DE row doubles as the check that real ICU data is loaded: an
        // invariant-globalization build renders it as "1,234.50" and fails
        // here rather than reporting a number nobody can read the meaning of.
        foreach (var (name, _, expected) in Shapes)
        {
            var actual = Render(name);
            if (actual != expected)
                throw new InvalidOperationException(
                    $"shape '{name}' rendered '{actual}', expected '{expected}'");
        }

        // The POCO rows must render what the dictionary rows render, or the
        // two argument shapes are not two ways of saying the same thing.
        var poco = _smart.Format(_invariant, _selectors, _pocoArgs);
        if (poco != "Hello Alice, you have 3 items")
            throw new InvalidOperationException($"POCO row rendered '{poco}'");
    }

    /// <summary>Renders one named shape, for the setup check and for --verify.</summary>
    public string Render(string name) => name switch
    {
        "render_selectors" => _smart.Format(_invariant, _selectors, _dictArgs),
        "render_number_spec" => _smart.Format(_invariant, _numberSpec, _floatArgs),
        "render_number_spec_de" => _smart.Format(_german, _numberSpec, _floatArgs),
        "render_plural" => _smart.Format(_invariant, _plural, _dictArgs),
        "render_choose" => _smart.Format(_invariant, _choose, _dictArgs),
        "render_list" => _smart.Format(_invariant, _list, _dictArgs),
        "render_nested" => _smart.Format(_invariant, _nested, _dictArgs),
        _ => throw new ArgumentOutOfRangeException(nameof(name), name, "unknown shape"),
    };

    // -----------------------------------------------------------------------
    // Parse once, render many: the shape both libraries are built for.
    // -----------------------------------------------------------------------

    [Benchmark(Description = "render_selectors (dict)")]
    public string RenderSelectors() => _smart.Format(_invariant, _selectors, _dictArgs);

    [Benchmark(Description = "render_number_spec (dict)")]
    public string RenderNumberSpec() => _smart.Format(_invariant, _numberSpec, _floatArgs);

    [Benchmark(Description = "render_number_spec_de (dict)")]
    public string RenderNumberSpecDe() => _smart.Format(_german, _numberSpec, _floatArgs);

    [Benchmark(Description = "render_plural (dict)")]
    public string RenderPlural() => _smart.Format(_invariant, _plural, _dictArgs);

    [Benchmark(Description = "render_choose (dict)")]
    public string RenderChoose() => _smart.Format(_invariant, _choose, _dictArgs);

    [Benchmark(Description = "render_list (dict)")]
    public string RenderList() => _smart.Format(_invariant, _list, _dictArgs);

    [Benchmark(Description = "render_nested (dict)")]
    public string RenderNested() => _smart.Format(_invariant, _nested, _dictArgs);

    // -----------------------------------------------------------------------
    // Parsing, and parsing plus rendering. Neither library caches a parsed
    // format, so both rows reparse on every call on both sides.
    // -----------------------------------------------------------------------

    [Benchmark(Description = "parse_simple")]
    public Format ParseSimple() => _smart.Parser.ParseFormat(SelectorsTemplate);

    [Benchmark(Description = "format_oneshot (dict)")]
    public string FormatOneshot() => _smart.Format(_invariant, SelectorsTemplate, _dictArgs);

    // -----------------------------------------------------------------------
    // The same work with a plain class instead of a dictionary, so the
    // arguments go through ReflectionSource. This is what most .NET callers
    // write. The port has no equivalent: it is handed a value tree.
    // -----------------------------------------------------------------------

    [Benchmark(Description = "render_selectors (POCO, reflection)")]
    public string RenderSelectorsPoco() => _smart.Format(_invariant, _selectors, _pocoArgs);

    [Benchmark(Description = "render_nested (POCO, reflection)")]
    public string RenderNestedPoco() => _smart.Format(_invariant, _nested, _pocoArgs);

    [Benchmark(Description = "format_oneshot (POCO, reflection)")]
    public string FormatOneshotPoco() => _smart.Format(_invariant, SelectorsTemplate, _pocoArgs);

    /// <summary>
    /// The dictionary's contents as a class, for the reflection rows. The
    /// property types match what the dictionary holds, so the two argument
    /// shapes differ only in how a member is found.
    /// </summary>
    private sealed class Person
    {
        public string Name => "Alice";
        public long Count => 3L;
        public string Gender => "f";
        public object[] Items => ["sword", "shield", "potion"];
    }
}
