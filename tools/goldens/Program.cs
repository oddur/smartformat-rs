// Golden-output harness: renders a hardcoded case table with the real
// SmartFormat.NET library and writes the results as JSON to stdout.
//
// Regenerate with:  dotnet run --project tools/goldens > goldens/m1.json

using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using SmartFormat;
using SmartFormat.Core.Settings;

const string smartFormatVersion = "3.6.1";

var cases = new List<GoldenCase>();

Literals(cases);
Selectors(cases);
Alignment(cases);
Nesting(cases);
Numbers(cases);
Dates(cases);
Errors(cases);
StringMethods(cases);
FormatterOptions(cases);
ListIndex(cases);
SettingsCases(cases);
LazyEscapeCases(cases);

var duplicates = cases.GroupBy(c => c.Id).Where(g => g.Count() > 1).Select(g => g.Key).ToList();
if (duplicates.Count > 0)
    throw new InvalidOperationException("duplicate case ids: " + string.Join(", ", duplicates));

var formatters = new Dictionary<CaseSettings, SmartFormatter>();

var caseArray = new JsonArray();
foreach (var c in cases)
{
    var settings = c.Settings ?? CaseSettings.Default;
    if (!formatters.TryGetValue(settings, out var smart))
        formatters[settings] = smart = Smart.CreateDefaultSmartFormat(settings.ToSmartSettings());

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

    var node = new JsonObject
    {
        ["id"] = c.Id,
        ["template"] = c.Template,
        ["args"] = JsonNode.Parse(c.ArgsJson),
        ["culture"] = "",
    };
    if (c.Settings is { } custom) node["settings"] = custom.ToJson();
    node["expected"] = expected;
    caseArray.Add(node);
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
    // \u takes a UTF-16 code unit, so an astral character is spelled as a
    // surrogate pair and the two halves join in the output string.
    Add("escape-unicode-surrogate-pair", @"a\ud83d\ude00b");
    Add("escape-unicode-surrogate-pair-only", @"\ud83d\ude00");
    // A lone escaped surrogate joins only with an escaped partner: a literal
    // astral character that follows is a character of its own, not a low half.
    // .NET keeps the lone surrogate as a UTF-16 code unit, which Utf8JsonWriter
    // transcodes to U+FFFD on its way into the golden file — the same character
    // a Rust String can hold, so these cases do compare.
    Add("escape-unicode-lone-high-then-pair", "a\\uD83D\U0001F600");
    Add("escape-unicode-lone-low-then-pair", "a\\uDE00\U0001F600");
    Add("escape-unicode-high-then-non-surrogate", "a\\uD83DAb");
    // The four hex digits go through NumberStyles.HexNumber, which skips
    // leading and trailing whitespace inside the four-character window.
    Add("escape-unicode-leading-space", @"\u 123");
    Add("escape-unicode-leading-space-inline", @"x\u 123y");
    Add("escape-unicode-two-leading-spaces", @"\u  12");
    Add("escape-unicode-trailing-tab", "\\u123\tz");
    Add("escape-unicode-short-window", @"abc\u12");
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

    // The nullable operator is a property of the whole placeholder in 3.6.1:
    // `Source.HasNullableOperator` scans every selector, so an operator behind
    // the null value covers it too, and a *missing* member is still an error.
    const string nullCity = """{"Name":"Alice","City":null}""";
    const string joe = """{"Person":{"Name":"Joe"}}""";
    void Nullable(string id, string template, string argsJson) =>
        cases.Add(new GoldenCase("sel-" + id, template, argsJson));

    Nullable("nullable-later-selector-covers-null", "{City.Length?.Nope}", nullCity);
    Nullable("nullable-later-selector-missing-branch", "{City.Nope?.Deep}", nullCity);
    Nullable("nullable-empty-result-is-aligned", "[{City.Length?.Nope,6}]", nullCity);
    Nullable("nullable-after-non-null-member", "{Name.Length?.Nope}", nullCity);
    Nullable("nullable-missing-key", "{Person?.Nope}", joe);
    Nullable("nullable-missing-key-chained", "{Person?.Nope?.Deep}", joe);
    Nullable("nullable-missing-first-selector", "{Missing?.Name}", joe);
    Nullable("nullable-null-member-missing-key", "{Person?.Nope}", """{"Person":null}""");

    // Default formatting of a collection: .NET falls back to object.ToString()
    // and renders the CLR type name, which this port refuses to do.
    Nullable("default-format-empty-args", "{}", "[]");
    Nullable("default-format-list", "{0}", "[[1,2,3]]");
    Nullable("default-format-map", "{Person}", joe);

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

    // A non-finite value never reaches the specifier in .NET, so even a
    // specifier that would be rejected or is a custom pattern renders the
    // culture symbol.
    var nonFinite = new (string Slug, double Value)[]
    {
        ("nan", double.NaN), ("pos-inf", double.PositiveInfinity),
        ("neg-inf", double.NegativeInfinity),
    };
    var offSubsetSpecs = new (string Spec, string Slug)[]
    {
        ("#,##0.00", "custom-pattern"), ("Q", "unknown-letter"),
        ("F1000000000", "precision-overflow"), ("00", "custom-zeroes"),
    };
    foreach (var (valueSlug, value) in nonFinite)
    foreach (var (spec, specSlug) in offSubsetSpecs)
        cases.Add(new GoldenCase(
            $"num-double-nonfinite-{valueSlug}-{specSlug}",
            Placeholder(spec),
            "[" + JsonDouble(value) + "]"));

    // Deliberate error combos: integer-only specifiers applied to doubles.
    foreach (var (valueSlug, value) in doubles.Where(d => d.Slug is "0_1" or "2_5" or "nan"))
    foreach (var (spec, specSlug) in allSpecs.Where(s => integerOnly.Contains(s.Spec)))
        cases.Add(new GoldenCase(
            $"num-double-badspec-{valueSlug}-{specSlug}",
            Placeholder(spec),
            "[" + JsonDouble(value) + "]"));

    // Integers round half away from zero and doubles round half to even, which
    // only shows up where a value sits exactly on a midpoint of the requested
    // precision. Without these the two rounding modes could be swapped.
    var intMidpoints = new (string Slug, long Value, string Spec)[]
    {
        ("1050-G3", 1050L, "G3"),
        ("1050-E1", 1050L, "E1"),
        ("1150-G2", 1150L, "G2"),
        ("2500-E0", 2500L, "E0"),
        ("neg-2500-E0", -2500L, "E0"),
        ("25-G1", 25L, "G1"),
        ("35-G1", 35L, "G1"),
    };
    foreach (var (slug, value, spec) in intMidpoints)
        cases.Add(new GoldenCase($"num-int-midpoint-{slug}", Placeholder(spec), "[" + JsonLong(value) + "]"));

    // The exact binary expansion, past the 5 decimals the combinatorial cases
    // reach.
    var precise = new (string Slug, double Value, string Spec)[]
    {
        ("0_1-F20", 0.1, "F20"),
        ("2_675-F30", 2.675, "F30"),
        ("0_1-G17", 0.1, "G17"),
        ("0_1-E20", 0.1, "E20"),
        ("min-subnormal-G", 5e-324, "G"),
        ("min-subnormal-G17", 5e-324, "G17"),
        ("max-F2", double.MaxValue, "F2"),
        ("1e100-N0", 1e100, "N0"),
        ("1e16-G", 1e16, "G"),
        ("1e17-G", 1e17, "G"),
        ("1e16-none", 1e16, ""),
        ("1e17-none", 1e17, ""),
        // Exact powers of two: the shortest-round-trip digits .NET produces
        // for these are one digit shorter than the correctly rounded form.
        ("pow2-neg25-none", 2.9802322387695312e-8, ""),
        ("pow2-neg25-G", 2.9802322387695312e-8, "G"),
        ("neg-pow2-neg25-none", -2.9802322387695312e-8, ""),
        ("pow2-neg958-none", 4.1045368012983762e-289, ""),
        ("pow2-neg958-G", 4.1045368012983762e-289, "G"),
        ("neg-pow2-neg958-none", -4.1045368012983762e-289, ""),
        ("pow2-neg24-none", 5.960464477539063e-8, ""),
        ("pow2-neg26-none", 1.4901161193847656e-8, ""),
        ("pow2-zero-none", 1.0, ""),
    };
    foreach (var (slug, value, spec) in precise)
        cases.Add(new GoldenCase($"num-double-precise-{slug}", Placeholder(spec), "[" + JsonDouble(value) + "]"));

    // Negative non-integers, where rounding and the negative currency/percent
    // patterns combine, plus the lowercase specifiers that must not change the
    // case of the output.
    var negatives = new (string Slug, double Value, string Spec)[]
    {
        ("C", -123.456, "C"),
        ("c1-lc", -123.456, "c1"),
        ("P1", -0.39678, "P1"),
        ("p-lc", -0.39678, "p"),
        ("N3", -1234.56, "N3"),
        ("n-lc", -1234.56, "n"),
        ("F0", -0.4, "F0"),
        ("f1-lc", -0.4, "f1"),
        ("G4", -1234.56, "G4"),
        ("E2", -1234.56, "E2"),
    };
    foreach (var (slug, value, spec) in negatives)
        cases.Add(new GoldenCase($"num-double-negative-{slug}", Placeholder(spec), "[" + JsonDouble(value) + "]"));

    var intSpecs = new (string Slug, long Value, string Spec)[]
    {
        ("d5-lc", -1234L, "d5"),
        ("X20", -255L, "X20"),
        ("x20-lc", -255L, "x20"),
        ("X-neg", -255L, "X"),
        ("D0", -1234L, "D0"),
        ("C0-neg", -1234L, "C0"),
    };
    foreach (var (slug, value, spec) in intSpecs)
        cases.Add(new GoldenCase($"num-int-spec-{slug}", Placeholder(spec), "[" + JsonLong(value) + "]"));

    // `R` (round-trip) and `B` (binary) are standard specifiers too.
    cases.Add(new GoldenCase("num-double-R-0_1", "{0:R}", "[" + JsonDouble(0.1) + "]"));
    cases.Add(new GoldenCase("num-double-r-lc-0_1", "{0:r}", "[" + JsonDouble(0.1) + "]"));
    cases.Add(new GoldenCase("num-double-R-2_675", "{0:R}", "[" + JsonDouble(2.675) + "]"));
    cases.Add(new GoldenCase("num-double-R-1e17", "{0:R}", "[" + JsonDouble(1e17) + "]"));
    cases.Add(new GoldenCase("num-int-R", "{0:R}", "[42]"));

    // `R` is rewritten as `(char)(format - ('R' - 'G'))`, so it *is* `G` with
    // the case kept — and only a floating-point value drops the precision.
    cases.Add(new GoldenCase("num-double-r-lc-1e17", "{0:r}", "[" + JsonDouble(1e17) + "]"));
    cases.Add(new GoldenCase("num-double-R5-1e17", "{0:R5}", "[" + JsonDouble(1e17) + "]"));
    cases.Add(new GoldenCase("num-double-r5-lc-1e17", "{0:r5}", "[" + JsonDouble(1e17) + "]"));
    cases.Add(new GoldenCase("num-double-r-lc-5e-324", "{0:r}", "[" + JsonDouble(5e-324) + "]"));
    cases.Add(new GoldenCase("num-double-R-1e-7", "{0:R}", "[" + JsonDouble(1e-7) + "]"));
    cases.Add(new GoldenCase("num-double-r5-lc-1e-7", "{0:r5}", "[" + JsonDouble(1e-7) + "]"));
    cases.Add(new GoldenCase("num-double-R20-2_675", "{0:R20}", "[" + JsonDouble(2.675) + "]"));
    cases.Add(new GoldenCase("num-int-R5", "{0:R5}", "[1234567890]"));
    cases.Add(new GoldenCase("num-int-r5-lc", "{0:r5}", "[1234567890]"));
    cases.Add(new GoldenCase("num-int-R0", "{0:R0}", "[1234567890]"));
    cases.Add(new GoldenCase("num-int-R20", "{0:R20}", "[1234567890]"));
    cases.Add(new GoldenCase("num-int-r-lc", "{0:r}", "[42]"));
    cases.Add(new GoldenCase("num-int-R5-neg", "{0:R5}", "[" + JsonLong(long.MinValue) + "]"));
    cases.Add(new GoldenCase("num-int-r5-lc-neg", "{0:r5}", "[" + JsonLong(long.MinValue) + "]"));

    // The one case where the CLR type of the boxed value shows: `X` on a
    // 32-bit int renders four bytes, where this port's `Value` has only i64.
    cases.Add(new GoldenCase("num-int32-X-neg", "{0:X}", """[{"$i32":"-255"}]"""));
    cases.Add(new GoldenCase("num-int32-B-neg", "{0:B}", """[{"$i32":"-5"}]"""));
    cases.Add(new GoldenCase("num-int-B", "{0:B}", "[5]"));
    cases.Add(new GoldenCase("num-int-B8", "{0:B8}", "[5]"));
    cases.Add(new GoldenCase("num-int-b-lc-neg", "{0:b}", "[-5]"));
    cases.Add(new GoldenCase("num-double-B", "{0:B}", "[" + JsonDouble(0.1) + "]"));

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

    // The lowercase aliases and the specifiers the combinatorial table misses,
    // plus years outside the four-digit-with-no-padding range.
    var extra = new (string Slug, string Spec, string RoundTrip)[]
    {
        ("2009-m-lc", "m", "2009-06-15T13:45:30.6175425"),
        ("2009-Y", "Y", "2009-06-15T13:45:30.6175425"),
        ("2009-o-lc", "o", "2009-06-15T13:45:30.6175425"),
        ("2009-r-lc", "r", "2009-06-15T13:45:30.6175425"),
        ("year-987-D", "D", "0987-11-09T00:00:00.0000000"),
        ("year-987-d-lc", "d", "0987-11-09T00:00:00.0000000"),
        ("year-987-y-lc", "y", "0987-11-09T00:00:00.0000000"),
        ("min-value-O", "O", "0001-01-01T00:00:00.0000000"),
        ("min-value-F", "F", "0001-01-01T00:00:00.0000000"),
        ("max-value-O", "O", "9999-12-31T23:59:59.9999999"),
        ("max-value-F", "F", "9999-12-31T23:59:59.9999999"),
        ("max-value-s-lc", "s", "9999-12-31T23:59:59.9999999"),
    };
    foreach (var (slug, spec, roundTrip) in extra)
        cases.Add(new GoldenCase(
            $"date-{slug}", "{0:" + spec + "}", $$"""[{"$dt":"{{roundTrip}}"}]"""));
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
    Add("trailing-operator-dot", "{0.}", one);
    Add("trailing-operator-after-member", "{0.Length.}", @"[""abc""]");

    // `\u` hands its four-character window to NumberStyles.HexNumber, which
    // accepts surrounding whitespace but nothing else outside [0-9A-Fa-f]. A
    // rejected window throws ArgumentException from EscapedLiteral.
    Add("unicode-escape-plus-sign", @"\u+123", empty);
    Add("unicode-escape-minus-sign", @"\u-123", empty);
    Add("unicode-escape-hex-prefix", @"\u0x12", empty);
    Add("unicode-escape-inner-space", @"\u12 3", empty);
    Add("unicode-escape-only-whitespace", @"\u    ", empty);
    // U+00A0 is whitespace to Unicode but not to .NET's number parser.
    Add("unicode-escape-unicode-whitespace", "\\u\u00A0123", empty);

    // Formatter options that never close: .NET indexes past the end of the
    // format string instead of reporting a parse error.
    Add("unterminated-formatter-options", "{0:d(", one);
    Add("unterminated-formatter-options-escape", @"{0:d(a\", one);
}

// ---------------------------------------------------------------------------
// StringSource selector methods
// ---------------------------------------------------------------------------

static void StringMethods(List<GoldenCase> cases)
{
    void Add(string id, string method, string value) =>
        cases.Add(new GoldenCase("str-" + id, "{0." + method + "}", JsonString(value)));

    Add("capitalize-words-digit-start", "CapitalizeWords", "1st place");
    Add("capitalize-words-punctuation-start", "CapitalizeWords", "(hello) world");
    Add("capitalize-words-plain", "CapitalizeWords", "hello world");
    Add("capitalize-words-already", "CapitalizeWords", "Hello World");
    Add("capitalize-words-multi-space", "CapitalizeWords", "a  b\tc");
    Add("capitalize-words-empty", "CapitalizeWords", "");
    Add("capitalize-empty", "Capitalize", "");
    Add("capitalize-single-char", "Capitalize", "a");
    Add("capitalize-already", "Capitalize", "Already");
    Add("capitalize-lowercase", "Capitalize", "abc");
    Add("capitalize-digit", "Capitalize", "1abc");
    Add("trim-start", "TrimStart", "  x  ");
    Add("trim-end", "TrimEnd", "  x  ");
    Add("to-upper-invariant", "ToUpperInvariant", "aBc");
    Add("to-lower-invariant", "ToLowerInvariant", "aBc");
    // .NET's ToUpper is a one-to-one mapping, so 'ß' stays 'ß'; Rust's full
    // mapping turns it into "SS".
    Add("to-upper-eszett", "ToUpper", "straße");
    Add("to-upper-invariant-eszett", "ToUpperInvariant", "straße");
    Add("to-lower-final-sigma", "ToLower", "ΩΣ");
}

// ---------------------------------------------------------------------------
// Formatter names and their options
// ---------------------------------------------------------------------------

static void FormatterOptions(List<GoldenCase> cases)
{
    void Add(string id, string template) => cases.Add(new GoldenCase("fopt-" + id, template, "[5]"));

    Add("empty-options", "{0:d()}");
    Add("escaped-colon", @"{0:d(a\:b)}");
    Add("escaped-close-paren", @"{0:d(a\)b)}");
    Add("options-then-format", "{0:d(x):v}");
    Add("name-without-options", "{0:d:v}");

    // A formatter name the parser abandons leaves its text in the format, so
    // these end with a nameless placeholder to make that text observable
    // instead of feeding it to the value as a custom numeric pattern.
    Add("empty-name", "{0:(){}}");
    Add("unescaped-colon-abandons", "{0:d(a:b){}}");
    Add("close-paren-not-followed-by-terminator", "{0:d(a)b{}}");
    Add("escaped-colon-with-nested", @"{0:d(a\:b):<{}>}");
}

// ---------------------------------------------------------------------------
// The list-index operator
// ---------------------------------------------------------------------------

static void ListIndex(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson) =>
        cases.Add(new GoldenCase("list-" + id, template, argsJson));

    const string nested = "[[1,2,3]]";
    const string dict = """{"a":[1,2,3],"n":null,"one":1}""";

    Add("positional-bracket", "{0[1]}", nested);
    Add("positional-dotted", "{0.1}", nested);
    Add("named-bracket", "{a[1]}", dict);
    Add("named-dotted", "{a.2}", dict);
    Add("named-bracket-out-of-range", "{a[9]}", dict);
    Add("nullable-bracket-on-null", "{n?[1]}", dict);
    Add("trailing-nullable-operator", "{one?}", dict);
    Add("bracket-then-member", "{0[1].Length}", """[["x","yy"]]""");
    Add("bracket-then-nested-format", "{0[1]:<{}>}", """[["x","yy"]]""");
}

// ---------------------------------------------------------------------------
// Non-default SmartSettings
// ---------------------------------------------------------------------------

static void SettingsCases(List<GoldenCase> cases)
{
    const string other = """{"Other":1}""";
    const string ab = """{"a":"A","b":"B"}""";
    const string one = "[42]";

    // FormatErrorAction against selector failures, a missing formatter and a
    // bad numeric specifier.
    var failing = new (string Slug, string Template, string Args)[]
    {
        ("missing-selector", "[{Missing}]", other),
        ("missing-selector-aligned", "[{Missing,05}]", other),
        ("missing-selector-neg-aligned", "[{Missing,-8}]", other),
        ("missing-selector-formatter-options", @"[{Missing:d(a\:b)}]", other),
        ("unknown-formatter", "[{0:nosuchformatter:x}]", one),
        ("bad-numeric-spec", "[{0:Q}]", one),
        ("member-on-int", "[{0.Nope}]", one),
        ("index-out-of-range", "[{5}]", one),
        ("no-error", "[{0:D3}]", one),
    };
    foreach (var action in new[]
             {
                 FormatErrorAction.Ignore, FormatErrorAction.MaintainTokens,
                 FormatErrorAction.OutputErrorInResult,
             })
    foreach (var (slug, template, args) in failing)
        cases.Add(new GoldenCase(
            $"set-fmterr-{action.ToString().ToLowerInvariant()}-{slug}", template, args,
            new CaseSettings(FormatErrorAction: action)));

    // ParseErrorAction against the six syntax errors the parser reports.
    var broken = new (string Slug, string Template)[]
    {
        ("invalid-selector-char", "a{b c}d"),
        ("trailing-operator", "a{b.}d"),
        ("too-many-closing-braces", "a}b"),
        ("missing-closing-brace-nested", "x{a:{b}"),
        ("too-many-closing-braces-after-format", "x{a:y}}z"),
        ("missing-closing-brace-trailing", "{a}{b"),
    };
    foreach (var action in new[]
             {
                 ParseErrorAction.Ignore, ParseErrorAction.MaintainTokens,
                 ParseErrorAction.OutputErrorInResult,
             })
    foreach (var (slug, template) in broken)
        cases.Add(new GoldenCase(
            $"set-parseerr-{action.ToString().ToLowerInvariant()}-{slug}", template, ab,
            new CaseSettings(ParseErrorAction: action)));

    // The caret of the OutputErrorInResult report counts UTF-16 code units, so
    // it keeps lining up under the template printed on the line above it.
    var outputParseError = new CaseSettings(ParseErrorAction: ParseErrorAction.OutputErrorInResult);
    cases.Add(new GoldenCase(
        "set-parseerr-outputerrorinresult-invalid-selector-char-nonascii",
        "äöü{a b}", ab, outputParseError));
    cases.Add(new GoldenCase(
        "set-parseerr-outputerrorinresult-invalid-selector-char-astral",
        "\U0001F600{a b}", ab, outputParseError));
    cases.Add(new GoldenCase(
        "set-parseerr-outputerrorinresult-too-many-closing-braces-astral",
        "\U0001F6000}", ab, outputParseError));

    // A custom numeric pattern renders in .NET and is a documented non-goal
    // here, so under OutputErrorInResult the two write different text.
    cases.Add(new GoldenCase(
        "set-fmterr-outputerrorinresult-custom-pattern", "[{0:#,##0.00}]",
        "[" + JsonDouble(1234.5) + "]",
        new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult)));

    // CaseSensitivity.
    var insensitive = new CaseSettings(CaseSensitivity: CaseSensitivityType.CaseInsensitive);
    const string person = """{"Name":"Alice","Text":"  hello world  "}""";
    cases.Add(new GoldenCase("set-caseins-map-key", "{name}", person, insensitive));
    cases.Add(new GoldenCase("set-caseins-map-key-upper", "{NAME}", person, insensitive));
    cases.Add(new GoldenCase("set-caseins-string-method", "{Name.tolower}", person, insensitive));
    cases.Add(new GoldenCase("set-caseins-formatter-name", "{Name:D():x}", person, insensitive));
    cases.Add(new GoldenCase("set-sensitive-map-key", "{name}", person));

    // Two case variants of one key. .NET takes the first insertion-order
    // ignore-case match whatever the selector spells; we prefer the exactly
    // spelled key, which agrees only for the first of these two.
    const string variants = """{"Name":"exact","NAME":"upper"}""";
    cases.Add(new GoldenCase("set-case-insensitive-exact-key-wins", "{Name}", variants, insensitive));
    cases.Add(new GoldenCase("set-case-insensitive-later-variant", "{NAME}", variants, insensitive));

    // OrdinalIgnoreCase folds non-ASCII, which needs the letters allowed in a
    // selector first.
    var umlaut = new CaseSettings(
        CaseSensitivity: CaseSensitivityType.CaseInsensitive, CustomSelectorChars: "Ää");
    cases.Add(new GoldenCase(
        "set-caseins-non-ascii", "{Ä}", """{"ä":"v"}""", umlaut));

    // StringFormatCompatibility: doubled braces escape, formatter names are
    // not parsed, and only DefaultFormatter runs.
    var compat = new CaseSettings(StringFormatCompatibility: true);
    cases.Add(new GoldenCase("set-compat-doubled-braces", "{{0}} {0}", "[5]", compat));
    cases.Add(new GoldenCase("set-compat-char-literal", @"a\nb", "[]", compat));
    cases.Add(new GoldenCase("set-compat-numeric-spec", "{0:d}", "[5]", compat));
    cases.Add(new GoldenCase("set-compat-formatter-name", "{0:d(x):v}", "[5]", compat));
    cases.Add(new GoldenCase("set-compat-date-spec", "{0:d}", """[{"$dt":"2009-06-15T13:45:30.0000000"}]""", compat));

    // AlignmentFillCharacter.
    var dotFill = new CaseSettings(AlignmentFillCharacter: '.');
    cases.Add(new GoldenCase("set-fill-right", "[{0,6}]", @"[""ab""]", dotFill));
    cases.Add(new GoldenCase("set-fill-left", "[{0,-6}]", @"[""ab""]", dotFill));
    cases.Add(new GoldenCase("set-fill-literal-in-nested", "[{0,6:<{}>}]", @"[""ab""]", dotFill));
}

// ---------------------------------------------------------------------------
// Lazy escape resolution: .NET only resolves escape sequences when the text is
// used — an unresolvable one in never-read formatter options renders fine, one
// in a written literal throws ArgumentException outside the error actions.
// ---------------------------------------------------------------------------

static void LazyEscapeCases(List<GoldenCase> cases)
{
    const string other = """{"Other":1}""";
    var parseIgnore = new CaseSettings(ParseErrorAction: ParseErrorAction.Ignore);
    var parseMaintain = new CaseSettings(ParseErrorAction: ParseErrorAction.MaintainTokens);
    var fmtIgnore = new CaseSettings(FormatErrorAction: FormatErrorAction.Ignore);
    var fmtOutput = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);
    var fmtMaintain = new CaseSettings(FormatErrorAction: FormatErrorAction.MaintainTokens);

    // Options and specifiers no formatter reads are never resolved.
    cases.Add(new GoldenCase("fopt-unresolvable-escape", @"{0:d(a\qb)}", "[7]"));
    cases.Add(new GoldenCase("fopt-unresolvable-escape-with-format", @"{0:d(a\qb):x}", @"[""S""]"));
    cases.Add(new GoldenCase("lit-unresolvable-escape-in-specifier", @"{0:a\qb}", @"[""X""]"));
    cases.Add(new GoldenCase("lit-unresolvable-unicode-escape-in-specifier", @"{0:a\uzzzzb}", @"[""S""]"));

    // A bad escape in written literal text throws whatever the error action;
    // a trailing escape character throws from the parser itself.
    cases.Add(new GoldenCase("set-parseerr-ignore-invalid-escape-sequence", @"a\db", "[]", parseIgnore));
    cases.Add(new GoldenCase("set-fmterr-ignore-invalid-escape-sequence", @"a\db", "[]", fmtIgnore));
    cases.Add(new GoldenCase("set-parseerr-maintaintokens-trailing-backslash", @"abc\", "[]", parseMaintain));

    // Inside a placeholder's format the escape resolves at write time, so the
    // format error action applies like any other formatting error.
    cases.Add(new GoldenCase("nest-invalid-escape-sequence", @"{0:{}a\qb}", "[7]"));
    cases.Add(new GoldenCase("set-fmterr-ignore-nested-invalid-escape", @"{0:{}a\qb}", "[7]", fmtIgnore));
    cases.Add(new GoldenCase(
        "set-fmterr-outputerrorinresult-nested-invalid-escape", @"{0:{}a\qb}", "[7]", fmtOutput));
    cases.Add(new GoldenCase(
        "set-fmterr-maintaintokens-nested-invalid-escape", @"{0:{}a\qb}", "[7]", fmtMaintain));

    // A failing placeholder resolves its formatter options while building the
    // error report, so a bad escape there trumps the lenient action...
    cases.Add(new GoldenCase(
        "set-fmterr-ignore-missing-selector-unresolvable-options",
        @"[{Missing:d(a\qb)}]", other, fmtIgnore));
    // ...but MaintainTokens reconstructs from raw text without resolving.
    cases.Add(new GoldenCase(
        "set-fmterr-maintaintokens-missing-selector-bad-format-escape",
        @"[{Missing:a\qb}]", other, fmtMaintain));
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
            if (obj.Count == 1 && obj.TryGetPropertyValue("$i32", out var i32))
                return int.Parse((string) i32!, NumberStyles.Integer, CultureInfo.InvariantCulture);

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

// A single string wrapped as the one positional argument of a case.
static string JsonString(string value) => "[" + JsonSerializer.Serialize(value) + "]";

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

internal readonly record struct GoldenCase(
    string Id, string Template, string ArgsJson, CaseSettings? Settings = null);

/// <summary>
/// The non-default <see cref="SmartSettings"/> a case runs with. A case without
/// one runs with <see cref="Default"/>, which is what SmartFormat.NET ships.
/// </summary>
internal sealed record CaseSettings(
    FormatErrorAction FormatErrorAction = FormatErrorAction.ThrowError,
    ParseErrorAction ParseErrorAction = ParseErrorAction.ThrowError,
    CaseSensitivityType CaseSensitivity = CaseSensitivityType.CaseSensitive,
    bool StringFormatCompatibility = false,
    char AlignmentFillCharacter = ' ',
    string CustomSelectorChars = "")
{
    public static readonly CaseSettings Default = new();

    public SmartSettings ToSmartSettings()
    {
        var settings = new SmartSettings
        {
            CaseSensitivity = CaseSensitivity,
            StringFormatCompatibility = StringFormatCompatibility,
            Formatter =
            {
                ErrorAction = FormatErrorAction,
                AlignmentFillCharacter = AlignmentFillCharacter,
            },
            Parser = { ErrorAction = ParseErrorAction },
        };
        if (CustomSelectorChars.Length > 0)
            settings.Parser.AddCustomSelectorChars(CustomSelectorChars.ToCharArray());
        return settings;
    }

    /// <summary>Only the properties that differ from the .NET defaults.</summary>
    public JsonObject ToJson()
    {
        var json = new JsonObject();
        if (FormatErrorAction != Default.FormatErrorAction)
            json["formatErrorAction"] = FormatErrorAction.ToString();
        if (ParseErrorAction != Default.ParseErrorAction)
            json["parseErrorAction"] = ParseErrorAction.ToString();
        if (CaseSensitivity != Default.CaseSensitivity)
            json["caseSensitivity"] = CaseSensitivity.ToString();
        if (StringFormatCompatibility != Default.StringFormatCompatibility)
            json["stringFormatCompatibility"] = StringFormatCompatibility;
        if (AlignmentFillCharacter != Default.AlignmentFillCharacter)
            json["alignmentFillCharacter"] = AlignmentFillCharacter.ToString();
        if (CustomSelectorChars != Default.CustomSelectorChars)
            json["customSelectorChars"] = CustomSelectorChars;
        return json;
    }
}
