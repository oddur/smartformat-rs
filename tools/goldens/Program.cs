// Golden-output harness: renders a hardcoded case table with the real
// SmartFormat.NET library and writes the results as JSON to stdout.
//
// Regenerate with:  dotnet run --project tools/goldens > goldens/m1.json

using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using SmartFormat;

const string smartFormatVersion = "3.6.1";

var cases = new List<GoldenCase>();

Literals(cases);
Selectors(cases);
Alignment(cases);
Nesting(cases);
Numbers(cases);
Dates(cases);
Errors(cases);

var duplicates = cases.GroupBy(c => c.Id).Where(g => g.Count() > 1).Select(g => g.Key).ToList();
if (duplicates.Count > 0)
    throw new InvalidOperationException("duplicate case ids: " + string.Join(", ", duplicates));

var smart = Smart.CreateDefaultSmartFormat();

var caseArray = new JsonArray();
foreach (var c in cases)
{
    var expected = new JsonObject();
    try
    {
        var caseArgs = JsonNode.Parse(c.ArgsJson);
        var result = caseArgs is JsonArray array
            ? smart.Format(CultureInfo.InvariantCulture, c.Template, ToPositionalArgs(array))
            : smart.Format(CultureInfo.InvariantCulture, c.Template, ToClrValue(caseArgs));
        expected["result"] = result;
    }
    catch (Exception ex)
    {
        expected["error"] = ex.GetType().Name;
    }

    caseArray.Add(new JsonObject
    {
        ["id"] = c.Id,
        ["template"] = c.Template,
        ["args"] = JsonNode.Parse(c.ArgsJson),
        ["culture"] = "",
        ["expected"] = expected,
    });
}

var document = new JsonObject
{
    ["smartformat_net_version"] = smartFormatVersion,
    ["culture"] = "InvariantCulture",
    ["cases"] = caseArray,
};

using var stdout = Console.OpenStandardOutput();
using (var writer = new Utf8JsonWriter(stdout, new JsonWriterOptions
       {
           Indented = true,
           Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
       }))
{
    document.WriteTo(writer);
}

stdout.Write("\n"u8);

// ---------------------------------------------------------------------------
// Case table
// ---------------------------------------------------------------------------

static void Literals(List<GoldenCase> cases)
{
    const string none = "[]";

    void Add(string id, string template) => cases.Add(new GoldenCase("lit-" + id, template, none));

    Add("empty", "");
    Add("plain", "Hello, World!");
    Add("whitespace-only", "   ");
    Add("percent", "100% done");
    Add("punctuation", "a-b_c/d");
    Add("escape-newline", @"a\nb");
    Add("escape-tab", @"a\tb");
    Add("escape-carriage-return", @"a\rb");
    Add("escape-backslash", @"a\\b");
    Add("escape-open-brace", @"a\{b");
    Add("escape-close-brace", @"a\}b");
    Add("escape-both-braces", @"\{\}");
    Add("escape-colon", @"a\:b");
    Add("escape-null-char", @"a\0b");
    Add("escape-alert", @"a\ab");
    Add("escape-backspace", @"a\bb");
    Add("escape-form-feed", @"a\fb");
    Add("escape-vertical-tab", @"a\vb");
    Add("escape-unicode-bullet", @"a\u2022b");
    Add("escape-unicode-e-acute", @"a\u00e9b");
    Add("escape-mixed", @"line1\nline2\ttabbed\\end");
    Add("escape-only-newline", @"\n");
    Add("trailing-backslash", @"abc\");
    Add("backslash-then-space", @"a\ b");
    Add("escaped-braces-no-placeholder", @"\{no placeholder\}");
    Add("unicode-in-template", "caf\u00e9 \u2022 na\u00efve");

    cases.Add(new GoldenCase("lit-escaped-braces-around-placeholder", @"\{{0}\}", "[1]"));
    cases.Add(new GoldenCase("lit-text-around-placeholder", "before {0} after", @"[""X""]"));
    cases.Add(new GoldenCase("lit-escape-newline-around-placeholder", @"{0}\n{0}", @"[""X""]"));
    cases.Add(new GoldenCase("lit-doubled-braces", "{{0}}", "[1]"));
}

static void Selectors(List<GoldenCase> cases)
{
    const string person = """
        {"Name":"Alice","Age":30,"City":null,"Empty":"","Text":"  hello world  ",
         "Person":{"Name":"Bob","Address":{"City":"Paris","Zip":"75001"}}}
        """;
    const string positional = """["zero", 1, 2.5, true, null]""";

    void Named(string id, string template) => cases.Add(new GoldenCase("sel-" + id, template, person));
    void Positional(string id, string template) => cases.Add(new GoldenCase("sel-" + id, template, positional));

    Named("named-string", "{Name}");
    Named("named-int", "{Age}");
    Named("named-null", "{City}");
    Named("named-empty-string", "{Empty}");
    Named("named-two", "{Name} is {Age}");
    Named("named-wrong-case", "{name}");
    Named("named-missing", "{Missing}");
    Named("nested-depth-2", "{Person.Name}");
    Named("nested-depth-3-city", "{Person.Address.City}");
    Named("nested-depth-3-zip", "{Person.Address.Zip}");
    Named("nested-missing-leaf", "{Person.Nope}");
    Named("nested-missing-branch", "{Nope.Name}");
    Named("null-member-without-nullable", "{City.Length}");
    Named("nullable-on-null", "{City?.Length}");
    Named("nullable-on-null-chained", "{City?.Length?.Nope}");
    Named("nullable-on-non-null", "{Name?.Length}");
    Named("nullable-deep-non-null", "{Person?.Address?.City}");
    Named("string-length", "{Name.Length}");
    Named("string-length-empty", "{Empty.Length}");
    Named("string-length-formatted", "{Name.Length:D3}");
    Named("string-to-upper", "{Name.ToUpper}");
    Named("string-to-lower", "{Name.ToLower}");
    Named("string-trim", "{Text.Trim}");
    Named("string-trim-length", "{Text.Trim.Length}");
    Named("int-length-invalid", "{Age.Length}");
    Named("nameless-self", "{Name:{}}");
    Named("nameless-self-twice", "{Name:{}-{}}");
    Named("nested-scope-one-level", "{Person:{Name}}");
    Named("nested-scope-two-levels", "{Person:{Address:{City}, {Zip}}}");
    Named("nested-scope-with-literal", "{Person:[{Name}]}");
    Named("nested-scope-nameless-inside", "{Person:{Name:{}}}");
    Named("nested-scope-dotted-inside", "{Person:{Address.City}}");
    Named("nested-scope-outer-selector-inside", "{Person:{Name} and {Age}}");

    Positional("positional-0", "{0}");
    Positional("positional-1", "{1}");
    Positional("positional-2", "{2}");
    Positional("positional-3-bool", "{3}");
    Positional("positional-4-null", "{4}");
    Positional("positional-out-of-range", "{5}");
    Positional("positional-sequence", "{0}-{1}-{2}");
    Positional("positional-repeat", "{0}{0}{0}");
    Positional("positional-reverse", "{2} {1} {0}");
    Positional("positional-string-length", "{0.Length}");
    Positional("positional-nameless-self", "{0:{}}");
    Positional("positional-negative-index", "{-1}");
    Positional("double-dot-empty-selector", "{0..Length}");
    Positional("format-spec-ignored-on-string", "{0:D5}");
}

static void Alignment(List<GoldenCase> cases)
{
    const string values = """["ab", "abcdefghijklmno", 42, 2.5, null, ""]""";

    var slots = new (string Slug, int Index)[]
    {
        ("short", 0), ("long", 1), ("null", 4), ("empty", 5),
    };
    var widths = new (string Slug, string Text)[]
    {
        ("r10", "10"), ("l10", "-10"), ("r3", "3"), ("l3", "-3"), ("zero", "0"),
    };

    foreach (var (valueSlug, index) in slots)
    foreach (var (widthSlug, widthText) in widths)
        cases.Add(new GoldenCase(
            $"align-{valueSlug}-{widthSlug}",
            $"[{{{index},{widthText}}}]",
            values));

    void Add(string id, string template) => cases.Add(new GoldenCase("align-" + id, template, values));

    Add("with-format-d5-right", "[{2,10:D5}]");
    Add("with-format-d5-left", "[{2,-10:D5}]");
    Add("with-format-currency-right", "[{3,12:C2}]");
    Add("with-format-currency-left", "[{3,-12:C2}]");
    Add("with-format-n3-right", "[{3,12:N3}]");
    Add("with-format-percent-left", "[{3,-12:P1}]");
    Add("with-format-narrower-than-value", "[{2,2:D5}]");
    Add("two-columns", "[{0,6}|{1,-6}]");
    Add("wide-right", "[{0,20}]");
    Add("wide-left", "[{0,-20}]");
    Add("explicit-plus", "[{0,+10}]");
    Add("space-before-width", "[{0, 10}]");
    Add("non-numeric-width", "[{0,x}]");
    Add("nameless-selector-with-width", "[{,10}]");
}

static void Nesting(List<GoldenCase> cases)
{
    const string data = """{"A":{"B":{"C":"deep"}},"Name":"Alice","Count":3,"N":2.5}""";

    void Add(string id, string template) => cases.Add(new GoldenCase("nest-" + id, template, data));

    Add("three-levels", "{A:{B:{C}}}");
    Add("three-levels-with-literals", "{A:<{B:<{C}>}>}");
    Add("mixed-dotted-and-nested", "{A:[{B.C}]}");
    Add("then-member", "{A:{B:{C.Length}}}");
    Add("with-format-spec", "{A:{B:{C.Length:D4}}}");
    Add("escaped-braces", @"{A:\{{B.C}\}}");
    Add("sibling-placeholders", "{Name:{}/{}}");
    Add("outer-and-inner-scope", "{A:{B:{C}} and {Name}}");
    Add("empty-format-string", "{Name:}");
    Add("empty-format-int", "{Count:}");
    Add("then-sibling-placeholder", "{A:{B:{C}}} x {Count:D2}");
    Add("repeated-placeholder", "{A:{B.C}-{B.C}}");
    Add("alignment-inside", "{A:[{B.C,10}]}");
    Add("alignment-outside", "{A,20:{B.C}}");
}

static void Numbers(List<GoldenCase> cases)
{
    var integers = new (string Slug, long Value)[]
    {
        ("zero", 0L),
        ("one", 1L),
        ("neg-one", -1L),
        ("42", 42L),
        ("i64-min", long.MinValue),
        ("i64-max", long.MaxValue),
    };

    var doubles = new (string Slug, double Value)[]
    {
        ("0_1", 0.1),
        ("0_125", 0.125),
        ("2_5", 2.5),
        ("2_675", 2.675),
        ("neg-zero", -0.0),
        ("1e15", 1e15),
        ("9_9999e14", 9.9999e14),
        ("1e-5", 1e-5),
        ("1e-4", 1e-4),
        ("nan", double.NaN),
        ("pos-inf", double.PositiveInfinity),
        ("neg-inf", double.NegativeInfinity),
    };

    // Lowercase specifiers get a "-lc" id slug so no two ids differ by case alone.
    var allSpecs = new (string Spec, string Slug)[]
    {
        ("", "none"), ("C", "C"), ("C0", "C0"), ("C3", "C3"),
        ("D", "D"), ("D8", "D8"),
        ("E", "E"), ("e2", "e2-lc"),
        ("F", "F"), ("F0", "F0"), ("F5", "F5"),
        ("G", "G"), ("G3", "G3"), ("g2", "g2-lc"),
        ("N", "N"), ("N0", "N0"),
        ("P", "P"), ("P1", "P1"),
        ("X", "X"), ("x8", "x8-lc"),
    };

    // "D" and "X" are integer-only specifiers in .NET.
    var integerOnly = new HashSet<string> { "D", "D8", "X", "x8" };

    foreach (var (valueSlug, value) in integers)
    foreach (var (spec, specSlug) in allSpecs)
        cases.Add(new GoldenCase(
            $"num-int-{valueSlug}-{specSlug}",
            Placeholder(spec),
            "[" + JsonLong(value) + "]"));

    foreach (var (valueSlug, value) in doubles)
    foreach (var (spec, specSlug) in allSpecs.Where(s => !integerOnly.Contains(s.Spec)))
        cases.Add(new GoldenCase(
            $"num-double-{valueSlug}-{specSlug}",
            Placeholder(spec),
            "[" + JsonDouble(value) + "]"));

    // Deliberate error combos: integer-only specifiers applied to doubles.
    foreach (var (valueSlug, value) in doubles.Where(d => d.Slug is "0_1" or "2_5" or "nan"))
    foreach (var (spec, specSlug) in allSpecs.Where(s => integerOnly.Contains(s.Spec)))
        cases.Add(new GoldenCase(
            $"num-double-badspec-{valueSlug}-{specSlug}",
            Placeholder(spec),
            "[" + JsonDouble(value) + "]"));

    static string Placeholder(string spec) => spec.Length == 0 ? "{0}" : "{0:" + spec + "}";
}

static void Dates(List<GoldenCase> cases)
{
    var dates = new (string Slug, string RoundTrip)[]
    {
        ("2009-fractional", "2009-06-15T13:45:30.6175425"),
        ("2024-leap-day", "2024-02-29T00:00:00.0000000"),
        ("1999-max-ticks", "1999-12-31T23:59:59.9999999"),
        ("2001-noon", "2001-01-01T12:00:00.0000000"),
    };

    var specs = new (string Spec, string Slug)[]
    {
        ("", "none"),
        ("d", "d-lc"), ("D", "D"), ("f", "f-lc"), ("F", "F"), ("g", "g-lc"), ("G", "G"),
        ("M", "M"), ("O", "O"), ("R", "R"), ("s", "s-lc"), ("t", "t-lc"), ("T", "T"),
        ("u", "u-lc"), ("y", "y-lc"),
    };

    foreach (var (dateSlug, roundTrip) in dates)
    foreach (var (spec, specSlug) in specs)
        cases.Add(new GoldenCase(
            $"date-{dateSlug}-{specSlug}",
            spec.Length == 0 ? "{0}" : "{0:" + spec + "}",
            $$"""[{"$dt":"{{roundTrip}}"}]"""));
}

static void Errors(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson) =>
        cases.Add(new GoldenCase("err-" + id, template, argsJson));

    const string one = "[42]";
    const string empty = "[]";

    Add("unclosed-placeholder", "{0", one);
    Add("unclosed-nested-placeholder", "{0:{0}", one);
    Add("lone-open-brace", "{", empty);
    Add("lone-close-brace", "}", empty);
    Add("unmatched-close-after-text", "abc}", empty);
    Add("extra-close-brace", "{0}}", one);
    Add("empty-selector-with-comma", "{,}", one);
    Add("space-inside-selector", "{0 1}", one);
    Add("braces-in-selector", "{ {0} }", one);
    Add("invalid-escape-sequence", @"a\db", empty);
    Add("invalid-unicode-escape", @"a\uzzzzb", empty);
    Add("no-args-but-placeholder", "{0}", empty);
    Add("unknown-formatter-name", "{0:nosuchformatter:x}", one);
    Add("invalid-numeric-spec", "{0:Q}", one);
    Add("selector-on-int", "{0.Nope}", one);
    Add("trailing-colon", "{0:", one);
}

// ---------------------------------------------------------------------------
// JSON -> CLR argument mapping (mirrored by the Rust golden runner)
// ---------------------------------------------------------------------------

static object?[] ToPositionalArgs(JsonArray array)
{
    var result = new object?[array.Count];
    for (var i = 0; i < array.Count; i++) result[i] = ToClrValue(array[i]);
    return result;
}

static object? ToClrValue(JsonNode? node)
{
    switch (node)
    {
        case null:
            return null;
        case JsonArray array:
            return ToPositionalArgs(array);
        case JsonObject obj:
        {
            if (obj.Count == 1 && obj.TryGetPropertyValue("$dt", out var dt))
                return DateTime.ParseExact(
                    (string) dt!, "O", CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind);
            if (obj.Count == 1 && obj.TryGetPropertyValue("$f", out var f))
                return double.Parse((string) f!, NumberStyles.Float, CultureInfo.InvariantCulture);

            var dict = new Dictionary<string, object?>();
            foreach (var (key, value) in obj) dict[key] = ToClrValue(value);
            return dict;
        }
        case JsonValue value:
        {
            var element = value.GetValue<JsonElement>();
            return element.ValueKind switch
            {
                JsonValueKind.True => true,
                JsonValueKind.False => false,
                JsonValueKind.String => element.GetString(),
                JsonValueKind.Null => null,
                // The cast keeps the conditional's type `object`; without it C#
                // would widen the long to double.
                JsonValueKind.Number => IsIntegerLiteral(element.GetRawText())
                    ? (object) element.GetInt64()
                    : element.GetDouble(),
                _ => throw new InvalidOperationException("unsupported JSON value: " + element),
            };
        }
        default:
            throw new InvalidOperationException("unsupported JSON node: " + node.GetType().Name);
    }
}

static bool IsIntegerLiteral(string rawNumber) => rawNumber.AsSpan().IndexOfAny(".eE") < 0;

static string JsonLong(long value) => value.ToString(CultureInfo.InvariantCulture);

// Doubles are written round-trippably, and always with a '.' or an exponent so
// a JSON reader can tell them apart from integers. NaN and the infinities have
// no JSON number form, so they use the "$f" marker object.
static string JsonDouble(double value)
{
    if (double.IsNaN(value)) return """{"$f":"NaN"}""";
    if (double.IsPositiveInfinity(value)) return """{"$f":"Infinity"}""";
    if (double.IsNegativeInfinity(value)) return """{"$f":"-Infinity"}""";

    var text = value.ToString("R", CultureInfo.InvariantCulture);
    return IsIntegerLiteral(text) ? text + ".0" : text;
}

internal readonly record struct GoldenCase(string Id, string Template, string ArgsJson);
