// Golden-output harness: renders a hardcoded case table with the real
// SmartFormat.NET library and writes the results as JSON to stdout.
//
// Regenerate with:  dotnet run --project tools/goldens > goldens/m1.json

using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using SmartFormat;
using SmartFormat.Core.Settings;
using SmartFormat.Extensions;
using SmartFormat.Extensions.PersistentVariables;
using SmartFormat.Utilities;

const string smartFormatVersion = "3.6.1";

// The wall clock every case that reads one reads. `TimeFormatter` on a
// `DateTime` and `ConditionalFormatter`'s date branch both go through
// `SystemTime.Now()`, which is a settable `Func<DateTime>`; the Rust port's
// stand-in is `SmartSettings::now`, and the runner reads this instant out of
// the document's `now` field. Kind is Unspecified, like every `$dt` argument,
// so .NET's `ToUniversalTime()` shifts a value and the clock by the same
// offset.
var pinnedNow = new DateTime(2026, 7, 31, 12, 0, 0, DateTimeKind.Unspecified);
SystemTime.SetDateTime(pinnedNow);

// Three extensions read the *thread* culture rather than the provider of the
// call — `ChooseFormatter` and `IsMatchFormatter` when they stringify a value,
// and `TimeFormatter` when it writes a unit's number — so the machine's locale
// would otherwise leak into the expected output. The invariant culture is the
// one the port behaves like: its negative sign is '-' and its group separator
// is ','.
CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
CultureInfo.CurrentUICulture = CultureInfo.InvariantCulture;

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
UnicodeEscapeSliceCases(cases);
PluralCases(cases);
ChooseCases(cases);
ConditionalCases(cases);
AutoDetectCases(cases);
CultureNumberCases(cases);
CultureDateCases(cases);
CultureNameCases(cases);
CultureFormatterCases(cases);
FormatterErrorTextCases(cases);
ListFormatterCases(cases);
SubStringCases(cases);
IsNullCases(cases);
IsMatchCases(cases);
TemplateCases(cases);
TimeCases(cases);
TimeSpanDefaultCases(cases);
LocalizationCases(cases);
VariablesCases(cases);
ClockConditionCases(cases);
// Must stay last: see the comment on the method.
CollectionIndexPoisoningCases(cases);

var duplicates = cases.GroupBy(c => c.Id).Where(g => g.Count() > 1).Select(g => g.Key).ToList();
if (duplicates.Count > 0)
    throw new InvalidOperationException("duplicate case ids: " + string.Join(", ", duplicates));

var formatters = new Dictionary<CaseSettings, SmartFormatter>();

// `ListFormatter.CollectionIndex` is a static, so a case that fails part-way
// through an iteration leaves it set for the *rest of the process* — every
// later case, whatever its settings and whatever formatter instance renders
// it, then sees that index instead of -1. The canary turns that silent
// corruption into a build failure: only the very last case may poison it.
var canary = Smart.CreateDefaultSmartFormat(new SmartSettings());
object?[] canaryArgs = [Array.Empty<object?>()];

var caseArray = new JsonArray();
for (var caseIndex = 0; caseIndex < cases.Count; caseIndex++)
{
    var c = cases[caseIndex];
    var settings = c.Settings ?? CaseSettings.Default;
    if (!formatters.TryGetValue(settings, out var smart))
        formatters[settings] = smart = BuildFormatter(settings);

    var culture = c.Culture.Length == 0
        ? CultureInfo.InvariantCulture
        : CultureInfo.GetCultureInfo(c.Culture);

    var expected = new JsonObject();
    try
    {
        var caseArgs = JsonNode.Parse(c.ArgsJson);
        var result = caseArgs is JsonArray array
            ? smart.Format(culture, c.Template, ToPositionalArgs(array))
            : smart.Format(culture, c.Template, ToClrValue(caseArgs));
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
        ["culture"] = c.Culture,
    };
    if (c.Settings is { } custom) node["settings"] = custom.ToJson();
    node["expected"] = expected;
    caseArray.Add(node);

    if (canary.Format(CultureInfo.InvariantCulture, "{Index}", canaryArgs) != "-1"
        && caseIndex != cases.Count - 1)
        throw new InvalidOperationException(
            $"case {c.Id} left ListFormatter.CollectionIndex set, which poisons every case " +
            "after it; move it into CollectionIndexPoisoningCases at the end of the table");
}

var document = new JsonObject
{
    ["smartformat_net_version"] = smartFormatVersion,
    ["default_culture"] = "InvariantCulture",
    ["now"] = pinnedNow.ToString("O", CultureInfo.InvariantCulture),
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
// The formatter a case runs with
// ---------------------------------------------------------------------------

/// <summary>
/// <c>Smart.CreateDefaultSmartFormat</c> plus whatever the case's settings ask
/// of the extensions that carry configuration of their own.
/// <see cref="TemplateFormatter"/> is not in the default set, so it is added
/// only for a case that names a template fixture; the Rust golden runner
/// mirrors all of this.
/// </summary>
static SmartFormatter BuildFormatter(CaseSettings settings)
{
    var smart = Smart.CreateDefaultSmartFormat(settings.ToSmartSettings());

    var isMatch = smart.GetFormatterExtension<IsMatchFormatter>()!;
    isMatch.RegexOptions = settings.RegexOptions;
    isMatch.SplitChar = settings.IsMatchSplitChar;
    isMatch.PlaceholderNameForMatches = settings.IsMatchPlaceholderName;
    isMatch.CanAutoDetect = settings.IsMatchCanAutoDetect;

    var subString = smart.GetFormatterExtension<SubStringFormatter>()!;
    subString.OutOfRangeBehavior = settings.SubStringOutOfRangeBehavior;
    subString.NullDisplayString = settings.SubStringNullDisplayString;
    subString.SplitChar = settings.SubStringSplitChar;
    subString.CanAutoDetect = settings.SubStringCanAutoDetect;

    var isNull = smart.GetFormatterExtension<NullFormatter>()!;
    isNull.SplitChar = settings.IsNullSplitChar;
    isNull.CanAutoDetect = settings.IsNullCanAutoDetect;

    var list = smart.GetFormatterExtension<ListFormatter>()!;
    list.SplitChar = settings.ListSplitChar;
    list.CanAutoDetect = settings.ListCanAutoDetect;

    // Neither of these two is in `CreateDefaultSmartFormat`, and neither can
    // ever auto-detect, so adding them to every formatter only makes the names
    // `time` and `L` resolvable. `AddExtensions` slots each one where
    // `WellKnownExtensionTypes` ranks it, which is what
    // `FormatterRegistry::add` does on the Rust side.
    smart.AddExtensions(new TimeFormatter());
    // The provider is a setting rather than a property of the formatter, and
    // `ToSmartSettings` has already put `LocalizationFixture.Provider` there.
    smart.AddExtensions(new LocalizationFormatter());

    // A variables source is *not* added to every formatter: it is ranked ahead
    // of every other source, so a group answers a selector before the argument
    // does, and a fixture holding a group named `Length` would change every
    // `{0.Length}` case in the table.
    if (settings.Variables != VariableSet.None)
        smart.AddExtensions(VariablesFixture(settings.Variables));

    if (settings.Templates != TemplateSet.None)
    {
        var templates = new TemplateFormatter();
        smart.AddExtensions(templates);
        foreach (var (name, template) in TemplateFixture(settings.Templates))
            templates.Register(name, template);
    }

    return smart;
}

/// <summary>
/// The named template sets a <c>template-*</c> case can ask for. .NET builds
/// the registry with the settings' case-sensitivity comparer and its
/// <c>Dictionary.Add</c> throws on a duplicate, so
/// <see cref="TemplateSet.CaseInsensitive"/> is the same fixture without the
/// <c>LAST</c> entry, which collides with <c>last</c> under OrdinalIgnoreCase.
/// </summary>
static (string Name, string Template)[] TemplateFixture(TemplateSet set)
{
    // The .NET test fixture, plus the odd names the escape cases resolve to
    // and the two templates the `cond` nesting case reaches.
    var standard = new (string Name, string Template)[]
    {
        ("firstLast", "{First} {Last}"),
        ("lastFirst", "{Last}, {First}"),
        ("FIRST", "{First.ToUpper}"),
        ("last", "{Last.ToLower}"),
        ("LAST", "{Last.ToUpper}"),
        ("NESTED", "{:t:FIRST} {:t:last}"),
        (@"back\slash", "BS"),
        ("{brace}", "BRACE"),
        ("a|b", "PIPE"),
        // Reads the list index, to see whether the scope chain survives being
        // re-entered through a template.
        ("indexed", "[{Index}] {First}"),
        ("salutation", "{1:cond:{:t:sal_formal}|{:t:sal_informal}}"),
        ("sal_formal", "Dear Mr {Last}"),
        ("sal_informal", "Hi {First}"),
        // A template whose own placeholder fails, for the error-report case.
        ("bad", "{Nope}"),
    };

    return set switch
    {
        TemplateSet.Standard => standard,
        // Nothing nested: `StringFormatCompatibility` turns off the syntax the
        // standard fixture is written in, and a template that does not parse
        // cannot be registered at all.
        TemplateSet.Simple => [("firstLast", "{First} {Last}"), ("x", "X-TEMPLATE")],
        // The empty name is a name like any other, and reaching it needs a
        // fixture where `{:t:}` is not the unknown-template error.
        TemplateSet.WithEmptyName => [.. standard, ("", "EMPTY")],
        TemplateSet.CaseInsensitive => [.. standard.Where(t => t.Name != "LAST")],
        _ => throw new InvalidOperationException("unknown template set: " + set),
    };
}

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
    cases.Add(new GoldenCase("set-fill-list-items", "[{0,4:list:{}|,}]", """[["a","b"]]""", dotFill));
    cases.Add(new GoldenCase("set-fill-substr", "[{0,6:substr(0,2)}]", @"[""abcd""]", dotFill));

    // The lenient error actions over the M3 formatters' own failures. The
    // M2 twins are in the `set-fmterr-*` table above; these are the shapes
    // only an M3 extension produces.
    var m3Failing = new (string Slug, string Template, string Args)[]
    {
        ("list-not-a-list", "[{0:list:{}|,}]", @"[""x""]"),
        ("list-one-part", "[{0:list:{}}]", """[["a","b"]]"""),
        ("substr-bad-option", "[{0:substr(x)}]", @"[""abcd""]"),
        ("substr-out-of-range", "[{0:substr(-99)}]", @"[""abcd""]"),
        ("substr-format-is-text", "[{0:substr(0,2):plain}]", @"[""abcd""]"),
        ("isnull-three-formats", "[{0:isnull:a|b|c}]", "[null]"),
    };
    foreach (var action in new[]
             {
                 FormatErrorAction.Ignore, FormatErrorAction.MaintainTokens,
                 FormatErrorAction.OutputErrorInResult,
             })
    foreach (var (slug, template, args) in m3Failing)
        cases.Add(new GoldenCase(
            $"set-fmterr-{action.ToString().ToLowerInvariant()}-{slug}", template, args,
            new CaseSettings(FormatErrorAction: action)));
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
// `\uXXXX` sequences the parser reads past.
//
// .NET's `Parser.ParseAlternativeEscaping` spans six characters for `\uXXXX`
// without checking that the four are hex digits, but then advances by *one*,
// so the four are read again as ordinary template text. Three consequences,
// all pinned below:
//
//   * a `|`, `{`, `}` or `\` among them is a real one — `{0:cond:a|\u12}` is a
//     closed placeholder and `x\u12{0}y` holds a nested placeholder;
//   * the literal run such a character ends starts *past* it, so .NET builds a
//     `LiteralText` whose `StartIndex` is after its `EndIndex`, and every
//     `Format.Split` over it asks `string.IndexOf` for a negative count and
//     throws `ArgumentOutOfRangeException` — for *every* argument;
//   * the same literal's source text reaches past the end of the format it
//     belongs to, so `Format.IndexOf` can also report a separator the format
//     does not cover. `Format.Substring` then throws
//     `ArgumentOutOfRangeException` for `start` or for `length` — but only when
//     the formatter asks for that one piece, because `SplitList` cuts lazily;
//   * a split can also cut a `\uXXXX` apart, and .NET resolves each piece
//     afresh, so a truncated sequence of one to three digits still resolves
//     (zero digits does not).
// ---------------------------------------------------------------------------

static void UnicodeEscapeSliceCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null,
             string culture = "") =>
        cases.Add(new GoldenCase("uesc-" + id, template, argsJson, settings, culture));

    var bothIgnore = new CaseSettings(
        FormatErrorAction: FormatErrorAction.Ignore, ParseErrorAction: ParseErrorAction.Ignore);
    var bothOutput = new CaseSettings(
        FormatErrorAction: FormatErrorAction.OutputErrorInResult,
        ParseErrorAction: ParseErrorAction.OutputErrorInResult);
    var bothMaintain = new CaseSettings(
        FormatErrorAction: FormatErrorAction.MaintainTokens,
        ParseErrorAction: ParseErrorAction.MaintainTokens);
    var noConvert = new CaseSettings(ConvertCharacterStringLiterals: false);
    var parseOutput = new CaseSettings(ParseErrorAction: ParseErrorAction.OutputErrorInResult);
    var fmtOutput = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);

    // -- A crossed literal fails for every argument, whichever piece is picked.
    Add("crossed-choose-1", @"{0:choose(1|2):\u|a\b}", "[1]");
    Add("crossed-choose-2", @"{0:choose(1|2):\u|a\b}", "[2]");
    Add("crossed-choose-ignore", @"{0:choose(1|2):\u|a\b}", "[2]", bothIgnore);
    Add("crossed-choose-output", @"{0:choose(1|2):\u|a\b}", "[2]", bothOutput);
    Add("crossed-cond-true", @"{0:cond:\u|a\b}", "[true]");
    Add("crossed-cond-false", @"{0:cond:\u|a\b}", "[false]");
    Add("crossed-plural-1", @"{0:plural:\u|a\b}", "[1]", culture: "en-US");
    Add("crossed-plural-2", @"{0:plural:\u|a\b}", "[2]", culture: "en-US");
    // The crossed literal is reached on the very first `IndexOf`, so neither
    // argument gets as far as choosing a piece.
    Add("crossed-choose-hex-1", @"{0:choose(1|2):\uAB\n|x}", "[1]");
    Add("crossed-choose-hex-2", @"{0:choose(1|2):\uAB\n|x}", "[2]");
    Add("crossed-cond-tail-1", @"{0:cond:a|\u1}", "[true]");
    Add("crossed-cond-tail-2", @"{0:cond:a|b\u}", "[true]");
    Add("crossed-cond-repeated", @"{0:cond:\u\u\u|b}", "[true]");

    // -- The `}` inside the escape window really closes the placeholder.
    Add("closing-brace-in-window-true", @"{0:cond:a|\u12}", "[true]");
    Add("closing-brace-in-window-false", @"{0:cond:a|\u12}", "[false]");
    Add("closing-brace-in-window-ignore", @"{0:cond:a|\u12}", "[true]", bothIgnore);
    Add("closing-brace-in-window-maintain", @"{0:cond:a|\u12}", "[true]", bothMaintain);
    Add("closing-brace-in-window-output", @"{0:cond:a|\u12}", "[true]", bothOutput);
    // A crossed literal is a fact about the parse, not about escape
    // resolution, so turning resolution off does not change the answer.
    Add("closing-brace-in-window-no-convert", @"{0:cond:a|\u12}", "[true]", noConvert);
    // Outside a placeholder the same `}` is one closing brace too many.
    Add("closing-brace-in-window-unbalanced", @"a\u12}", "[]");
    Add("closing-brace-in-window-unbalanced-output", @"a\u12}", "[]", parseOutput);
    Add("closing-brace-in-window-unbalanced-only", @"\u12}", "[]");

    // -- Zero hex digits never resolve: `int.TryParse` fails on the empty
    // slice. Only the piece that keeps the `\u` fails.
    Add("zero-digits-choose-1", @"{0:choose(1|2):\u|abcd}", "[1]");
    Add("zero-digits-choose-2", @"{0:choose(1|2):\u|abcd}", "[2]");
    Add("zero-digits-choose-output", @"{0:choose(1|2):\u|abcd}", "[1]", fmtOutput);
    Add("zero-digits-cond-true", @"{0:cond:\u|abcd}", "[true]");
    Add("zero-digits-cond-false", @"{0:cond:\u|abcd}", "[false]");
    Add("zero-digits-plural-1", @"{0:plural:\u|abcd}", "[1]", culture: "en-US");
    Add("zero-digits-plural-2", @"{0:plural:\u|abcd}", "[2]", culture: "en-US");

    // -- One to three digits do resolve, so a split through the hex window
    // gives two pieces that each mean something.
    Add("sliced-choose-first", @"{0:choose(1|2):x|\u00|41}", "[1]");
    Add("sliced-choose-second", @"{0:choose(1|2):x|\u00|41}", "[2]");
    Add("sliced-choose-third", @"{0:choose(1|2|3):x|\u00|41}", "[3]");
    Add("sliced-choose-one-digit-first", @"{0:choose(1|2):\u0|041}", "[1]");
    Add("sliced-choose-one-digit-second", @"{0:choose(1|2):\u0|041}", "[2]");
    Add("sliced-cond-true", @"{0:cond:\u00|41}", "[true]");
    Add("sliced-cond-false", @"{0:cond:\u00|41}", "[false]");
    // `NumberStyles.HexNumber` allows white space around the digits.
    Add("hex-window-space-none", @"{0:cond:a\u12b|c}", "[true]");
    Add("hex-window-space-inner", @"{0:cond:a\u1 2b|c}", "[true]");
    Add("hex-window-space-leading", @"{0:cond:a\u 12b|c}", "[true]");

    // -- A crossed literal the split walks past without ever asking for a
    // negative count: the middle piece is cut out of two literals that cover
    // the same character.
    Add("crossed-walked-past-1", @"{0:choose(1|2):\u1\|2|x}", "[1]");
    Add("crossed-walked-past-2", @"{0:choose(1|2):\u1\|2|x}", "[2]");
    Add("crossed-not-reached", @"{0:choose(1|2|3):a|\u12|c}", "[1]");

    // -- A brace inside the window opens a real placeholder, and the literal
    // that ends at it is unresolvable however it is written: the
    // `ArgumentException` comes from the parser, where no error action helps.
    Add("open-brace-in-window", @"x\u12{0}y", "[5]");
    Add("open-brace-in-window-only", @"\u12{0}", "[5]");
    Add("open-brace-in-window-nested", @"{0:cond:{1}\u12|b}", @"[true,""x""]");

    // -- Surrogate handling is untouched by any of this.
    Add("astral-literal", "a\U0001F600b", "[]");
    Add("astral-cond", "{0:cond:a\U0001F600|b}", "[true]");
    Add("astral-choose", "{0:choose(1|2):\U0001F600|b}", "[1]");
    Add("sliced-surrogate-pair", @"{0:cond:\uD83D\uDE0|b}", "[true]");

    // -- The split throws before the formatter counts the pieces, so a piece
    // count the formatter would have rejected never gets the chance to be
    // reported: the `OutputErrorInResult` twin of each is the Count message,
    // not "requires at least N format parameters".
    Add("crossed-cond-arity", @"{0:cond:\uAB\n}", "[true]");
    Add("crossed-cond-arity-output", @"{0:cond:\uAB\n}", "[true]", fmtOutput);
    Add("crossed-choose-arity", @"{0:choose(1|2|3):\uAB\n|x}", "[1]");
    Add("crossed-choose-arity-output", @"{0:choose(1|2|3):\uAB\n|x}", "[1]", fmtOutput);
    Add("crossed-plural-arity", @"{0:plural:\u\u}", "[1]", culture: "en-US");
    Add("crossed-plural-arity-output", @"{0:plural:\u\u}", "[1]", fmtOutput, "en-US");
    // A placeholder with no formatter name at all: .NET reaches the same
    // throw through ListFormatter, which splits the format of *every*
    // placeholder that has one before it looks at the value. We have no list
    // formatter yet and reach it through the plural formatter's
    // auto-detection, which also splits before it counts.
    Add("crossed-list-split", @"{0:\u12}", "[5]");
    Add("crossed-list-split-output", @"{0:\u12}", "[5]", fmtOutput);
    Add("crossed-list-split-nested", @"{0:\u12{1}}", "[5,6]");
    Add("crossed-list-split-nested-output", @"{0:\u12{1}}", "[5,6]", fmtOutput);

    // -- A separator the format does not cover. The `}` that closes the
    // placeholder is inside the escape window, so the format ends there while
    // the crossed literal's source text runs on to the `|` after it. The
    // search finds that `|` and hands `Format.Substring` a piece the format
    // does not cover: the piece before the separator overruns the end
    // (`length`), the piece after it starts past the end (`start`). Which one
    // is reported depends on which piece the formatter picks, since .NET only
    // cuts the piece it is asked for.
    Add("out-of-range-cond-true", @"{0:cond:\u12}|{1}", @"[true,""Z""]");
    Add("out-of-range-cond-false", @"{0:cond:\u12}|{1}", @"[false,""Z""]");
    Add("out-of-range-cond-true-output", @"{0:cond:\u12}|{1}", @"[true,""Z""]", fmtOutput);
    Add("out-of-range-cond-false-output", @"{0:cond:\u12}|{1}", @"[false,""Z""]", fmtOutput);
    Add("out-of-range-cond-leading-true", @"{0:cond:x\u12}|{1}", @"[true,""Z""]");
    Add("out-of-range-cond-leading-false", @"{0:cond:x\u12}|{1}", @"[false,""Z""]");
    Add("out-of-range-cond-maintain-true", @"{0:cond:\u12}|b}", "[true]", bothMaintain);
    Add("out-of-range-cond-maintain-false", @"{0:cond:\u12}|b}", "[false]", bothMaintain);
    Add("out-of-range-choose-1", @"{0:choose(1|2):\u12}|{1}", @"[1,""Z""]");
    Add("out-of-range-choose-2", @"{0:choose(1|2):\u12}|{1}", @"[2,""Z""]");
    Add("out-of-range-choose-1-output", @"{0:choose(1|2):\u12}|{1}", @"[1,""Z""]", fmtOutput);
    Add("out-of-range-choose-2-output", @"{0:choose(1|2):\u12}|{1}", @"[2,""Z""]", fmtOutput);
    Add("out-of-range-plural", @"{0:plural:\u12}|{1}", @"[1,""Z""]", culture: "en-US");
    Add("out-of-range-plural-output", @"{0:plural:\u12}|{1}", @"[1,""Z""]", fmtOutput, "en-US");

    // The pieces the format does cover still render: only the argument that
    // picks the piece out of bounds fails.
    Add("out-of-range-choose-in-range-1", @"{0:choose(1|2|3):a|b|\u12}|{1}", @"[1,""Z""]");
    Add("out-of-range-choose-in-range-2", @"{0:choose(1|2|3):a|b|\u12}|{1}", @"[2,""Z""]");
    Add("out-of-range-choose-in-range-3", @"{0:choose(1|2|3):a|b|\u12}|{1}", @"[3,""Z""]");
    Add("out-of-range-choose-in-range-3-output",
        @"{0:choose(1|2|3):a|b|\u12}|{1}", @"[3,""Z""]", fmtOutput);
    // A complex condition reads its parameters one at a time, so the piece out
    // of bounds only throws once the loop reaches it.
    Add("out-of-range-cond-complex-first", @"{0:cond:<5?a|\u12}|{1}", @"[1,""Z""]");
    Add("out-of-range-cond-complex-second", @"{0:cond:<5?a|\u12}|{1}", @"[9,""Z""]");
}

// ---------------------------------------------------------------------------
// M2: PluralLocalizationFormatter.
//
// One block per language, crossed with the counts that separate its rule's
// arms. The word lists say which arm was taken rather than being real
// translations, so a diff names the arm directly.
// ---------------------------------------------------------------------------

static void PluralCases(List<GoldenCase> cases)
{
    // The number of words is the one each language's rule expects: two for the
    // "one/other" languages, three for Russian, four for Polish, five for
    // Czech, six for Arabic, one for the singular languages.
    var languages = new (string Slug, string Culture, string Words)[]
    {
        ("en", "en-US", "one|other"),
        ("de", "de-DE", "one|other"),
        ("fr", "fr-FR", "one|other"),
        ("es", "es-ES", "one|other"),
        ("pt-br", "pt-BR", "one|other"),
        ("is", "is-IS", "one|other"),
        ("tr", "tr", "one|other"),
        ("ru", "ru", "one|few|other"),
        ("pl", "pl", "one|few|many|other"),
        ("cs", "cs", "zero|one|few|many|other"),
        ("ja", "ja", "all"),
        ("ko", "ko", "all"),
        ("zh", "zh-Hans", "all"),
        ("ar", "ar", "zero|one|two|few|many|other"),
    };

    var counts = new (string Slug, string Json)[]
    {
        ("0", "0"), ("1", "1"), ("2", "2"), ("3", "3"), ("5", "5"),
        ("11", "11"), ("21", "21"), ("22", "22"), ("100", "100"), ("101", "101"),
        ("1_5", "1.5"),
    };

    foreach (var (langSlug, culture, words) in languages)
    foreach (var (countSlug, json) in counts)
        cases.Add(new GoldenCase(
            $"plural-{langSlug}-{countSlug}",
            "{0:plural:" + words + "}",
            "[" + json + "]",
            Culture: culture));

    // Word counts other than the natural one: the "one/other" rule also serves
    // three words (zero/one/other) and four (negative/zero/one/other), and
    // French's rule has its own three- and four-word arms.
    var wordCountVariants = new (string Slug, string Culture, string Words)[]
    {
        ("en-3", "en-US", "zero|one|other"),
        ("en-4", "en-US", "neg|zero|one|other"),
        ("de-3", "de-DE", "zero|one|other"),
        ("fr-2", "fr-FR", "one|other"),
        ("fr-3", "fr-FR", "zero|one|other"),
        ("fr-4", "fr-FR", "neg|zero|one|other"),
    };
    var signedCounts = new (string Slug, string Json)[]
    {
        ("neg-2", "-2"), ("neg-1", "-1"), ("0", "0"), ("1", "1"),
        ("1_5", "1.5"), ("2", "2"), ("7", "7"),
    };
    foreach (var (slug, culture, words) in wordCountVariants)
    foreach (var (countSlug, json) in signedCounts)
        cases.Add(new GoldenCase(
            $"plural-words-{slug}-{countSlug}",
            "{0:plural:" + words + "}",
            "[" + json + "]",
            Culture: culture));

    // The language named in the formatter options wins over the culture of the
    // call; the culture here is the invariant one, whose language .NET's
    // PluralLocalizationFormatter defaults to "en".
    var namedLanguages = new (string Slug, string Option, string Words)[]
    {
        ("pl", "pl", "one|few|many|other"),
        ("ru", "ru", "one|few|other"),
        ("ar", "ar", "zero|one|two|few|many|other"),
        ("ja", "ja", "all"),
        ("upper-pl", "PL", "one|few|many|other"),
        ("region-pl", "pl-PL", "one|few|many|other"),
        ("spaced-pl", " pl ", "one|few|many|other"),
        ("kde", "kde", "all"),
    };
    foreach (var (slug, option, words) in namedLanguages)
    foreach (var count in new[] { "1", "2", "5", "22" })
        cases.Add(new GoldenCase(
            $"plural-option-{slug}-{count}",
            "{0:plural(" + option + "):" + words + "}",
            "[" + count + "]"));

    // Empty options fall back to the culture of the call.
    cases.Add(new GoldenCase("plural-option-empty", "{0:plural():one|other}", "[1]", Culture: "fr-FR"));
    cases.Add(new GoldenCase("plural-option-blank", "{0:plural( ):one|other}", "[2]", Culture: "fr-FR"));

    // The invariant culture pluralizes as English.
    cases.Add(new GoldenCase("plural-invariant-1", "{0:plural:one|other}", "[1]"));
    cases.Add(new GoldenCase("plural-invariant-2", "{0:plural:one|other}", "[2]"));

    // A list pluralizes by how many items it has.
    var lists = new (string Slug, string Json)[]
    {
        ("0", "[[]]"), ("1", "[[7]]"), ("2", "[[7,8]]"), ("5", "[[1,2,3,4,5]]"),
    };
    foreach (var (slug, json) in lists)
        cases.Add(new GoldenCase(
            $"plural-list-{slug}", "{0:plural:zero|one|other}", json, Culture: "en-US"));

    // The count renders through the culture as well, so the same case pins the
    // number formatting the plural word sits next to.
    foreach (var (culture, slug) in new[] { ("en-US", "en"), ("fr-FR", "fr"), ("ru", "ru"), ("de-DE", "de") })
    foreach (var (countSlug, json) in new[] { ("1", "1"), ("1_5", "1.5"), ("21", "21"), ("1234_5", "1234.5") })
        cases.Add(new GoldenCase(
            $"plural-with-value-{slug}-{countSlug}",
            "{0} {0:plural:item|items}",
            "[" + json + "]",
            Culture: culture));

    // Nesting, splitting and alignment.
    cases.Add(new GoldenCase("plural-nested-placeholder", "{0:plural:{0:plural:x|y}|c}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-nested-placeholder-other", "{0:plural:{0:plural:x|y}|c}", "[2]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-escaped-split-char", @"{0:plural:a\|b|c}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-escaped-split-char-other", @"{0:plural:a\|b|c}", "[2]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-escaped-newline", @"{0:plural:a\nb|c}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-escaped-brace", @"{0:plural:a\{b|c}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-empty-words", "[{0:plural:|}]", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-empty-first-word", "[{0:plural:|b}]", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-empty-second-word", "[{0:plural:a|}]", "[2]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-alignment-right", "[{0,10:plural:one|many}]", "[2]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-alignment-left", "[{0,-10:plural:one|many}]", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-other-separators", "{0:plural:a,b|c}", "[2]", Culture: "en-US"));

    // Values at the edge of Convert.ToDecimal.
    var edgeDoubles = new (string Slug, string Json)[]
    {
        ("1e28", "1e28"),
        ("pow96-minus", "7.922816251426433e28"),
        ("just-below-one", "0.9999999999999999"),
        ("just-above-one", "1.0000000000000002"),
    };
    foreach (var (slug, json) in edgeDoubles)
        cases.Add(new GoldenCase(
            $"plural-edge-{slug}", "{0:plural:one|other}", "[" + json + "]", Culture: "en-US"));

    // Values decimal cannot hold at all: .NET throws before the rule runs.
    foreach (var (slug, json) in new (string, string)[]
             {
                 ("1e29", "1e29"), ("neg-1e29", "-1e29"), ("pow96", "7.922816251426434e28"),
                 ("nan", """{"$f":"NaN"}"""), ("inf", """{"$f":"Infinity"}"""),
             })
        cases.Add(new GoldenCase(
            $"plural-overflow-{slug}", "{0:plural:one|other}", "[" + json + "]", Culture: "en-US"));

    // French rounds tiny values to zero the way decimal does.
    foreach (var (slug, json) in new (string, string)[]
             {
                 ("1e-30", "1e-30"), ("5e-29", "5e-29"), ("above-5e-29", "5.0000001e-29"), ("0_5", "0.5"),
             })
        cases.Add(new GoldenCase(
            $"plural-tiny-{slug}", "{0:plural:zero|one|other}", "[" + json + "]", Culture: "fr-FR"));

    // Errors.
    cases.Add(new GoldenCase("plural-err-one-word", "{0:plural:One}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-five-words", "{0:plural:a|b|c|d|e}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-string", "{0:plural:One|Two}", @"[""1234""]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-empty-string", "{0:plural:One|Two}", @"[""""]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-bool", "{0:plural:One|Two}", "[false]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-null", "{0:plural:One|Two}", "[null]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-map", "{0:plural:One|Two}", """{"a":1}""", Culture: "en-US"));
    // A culture name goes through CultureInfo.GetCultureInfo, so `_` separates
    // subtags just like `-`, and a malformed name is a CultureNotFoundException
    // wrapped in a FormattingException at index 0.
    cases.Add(new GoldenCase("plural-option-underscore-ru", "{0:plural(ru_RU):a|b|c}", "[1]"));
    cases.Add(new GoldenCase("plural-option-underscore-en", "{0:plural(en_US):one|many}", "[2]", Culture: "ru"));
    cases.Add(new GoldenCase("plural-option-long-name", "{0:plural(en-us-x-private):one|many}", "[2]", Culture: "ru"));
    // One `_` is an alternate sort order, so everything after it is dropped
    // and the language is still `en`.
    cases.Add(new GoldenCase(
        "plural-option-sort-order-subtag", "{0:plural(en_US-POSIX):one|many}", "[2]", Culture: "ru"));
    foreach (var (slug, name) in new (string, string)[]
             {
                 ("trailing-dash", "en-"), ("double-dash", "en--US"), ("leading-dash", "-en"),
                 ("trailing-underscore", "EN_"), ("two-underscores", "aa_bb_cc"),
                 ("punctuation", "@@"), ("space", "e n"), ("non-ascii", "ру"),
                 ("one-character", "a"), ("long-language", "aaaaaaaaaaaa"),
                 // Two `_`, which is one sort order too many.
                 ("en-us-posix", "en_US_POSIX"),
             })
        cases.Add(new GoldenCase(
            $"plural-err-culture-name-{slug}", "{0:plural(" + name + "):a|b}", "[1]", Culture: "en-US"));

    cases.Add(new GoldenCase("plural-err-unknown-language", "{0:plural(xx):a|b}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-language-without-rule", "{0:plural(hy):a|b}", "[1]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-short-name", "{0:p:one|many}", "[2]", Culture: "en-US"));
    cases.Add(new GoldenCase("plural-err-upper-name", "{0:PLURAL:one|many}", "[2]", Culture: "en-US"));

    // A split character among the hex digits of a `\uXXXX` sequence: .NET
    // resolves each piece of the split afresh, so the truncated `\u00` is a
    // NUL character and the stray `4` joins the `1` after the separator.
    cases.Add(new GoldenCase("plural-sliced-unicode-escape", @"{0:plural:\u00|41}", "[1]", Culture: "en-US"));

    // Divergences, held here with .NET's answer and skipped by the Rust runner.
    cases.Add(new GoldenCase(
        "plural-i64-beyond-double", "{0:plural(ru):a|b|c}", "[10000000000000001]", Culture: "en-US"));
    // The same loss of precision for a double: 1e28 is not 10^28 as an f64.
    cases.Add(new GoldenCase(
        "plural-f64-beyond-double", "{0:plural(ru):a|b|c}", "[1e28]", Culture: "en-US"));
    // ICU maps a three-letter ISO 639-2 code to its two-letter equivalent, so
    // this is English in .NET; we take the name as written and have no rule.
    cases.Add(new GoldenCase("plural-option-iso-639-2", "{0:plural(eng):one|many}", "[2]", Culture: "ru"));
    cases.Add(new GoldenCase("plural-bare-name", "{0:plural}", "[2]", Culture: "en-US"));
}

// ---------------------------------------------------------------------------
// M2: ChooseFormatter. Every case runs with the invariant culture, which is
// also the one .NET compares the options under.
// ---------------------------------------------------------------------------

static void ChooseCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("choose-" + id, template, argsJson, settings));

    // Matching by position, by spelling, and across the string/number divide.
    Add("int-1", "{0:choose(1|2|3):one|two|three}", "[1]");
    Add("int-2", "{0:choose(1|2|3):one|two|three}", "[2]");
    Add("int-3", "{0:choose(1|2|3):one|two|three}", "[3]");
    Add("int-reordered", "{0:choose(3|2|1):three|two|one}", "[1]");
    Add("string-digit", "{0:choose(1|2|3):one|two|three}", @"[""1""]");
    Add("string-letter", "{0:choose(A|B|C):Alpha|Bravo|Charlie}", @"[""B""]");

    // Bools and null match case-insensitively whatever the settings say.
    Add("bool-true", "{0:choose(True|False):yep|nope}", "[true]");
    Add("bool-false", "{0:choose(True|False):yep|nope}", "[false]");
    Add("bool-lowercase-option", "{0:choose(true|false):yep|nope}", "[true]");
    Add("bool-uppercase-option", "{0:choose(TRUE|FALSE):yep|nope}", "[false]");
    Add("null", "{0:choose(null):is null|default}", "[null]");
    Add("null-uppercase-option", "{0:choose(NULL):is null|default}", "[null]");

    // The else branch.
    Add("else-number", "{0:choose(1|2|3):one|two|three|default}", "[99]");
    Add("else-null", "{0:choose(1|2|3):one|two|three|default}", "[null]");
    Add("else-bool", "{0:choose(1|2|3):one|two|three|default}", "[true]");
    Add("else-string", "{0:choose(1|2|3):one|two|three|default}", @"[""whatever""]");
    Add("else-renders-value", "{0:choose(null):nothing|{}}", "[5]");
    Add("else-not-taken", "{0:choose(null):nothing|{}}", "[null]");
    Add("else-after-two", "{0:choose(null|5):nothing|five|{}}", "[6]");
    Add("else-after-two-match", "{0:choose(null|5):nothing|five|{}}", "[5]");
    Add("else-after-two-null", "{0:choose(null|5):nothing|five|{}}", "[null]");

    // Strings compare case-sensitively unless the formatter says otherwise.
    Add("case-sensitive-second", "{0:choose(string|String):one|two|default}", @"[""String""]");
    Add("case-sensitive-none", "{0:choose(string|STRING):one|two|default}", @"[""String""]");

    // Empty options.
    Add("empty-option", "{Input:choose(null|):A|B|{}}", """{"Input":""}""");
    Add("no-options-empty", "{0:choose():a|b}", @"[""""]");
    Add("no-options-other", "{0:choose():a|b}", @"[""x""]");

    // Nested placeholders and formats.
    const string nullable = """{"NullableInt":null,"IntValueIfNull":9999}""";
    const string nonNull = """{"NullableInt":1234,"IntValueIfNull":9999}""";
    Add("nested-format-null", "{NullableInt:choose(null):{IntValueIfNull:N2}|{:N2}}", nullable);
    Add("nested-format-value", "{NullableInt:choose(null):{IntValueIfNull:N2}|{:N2}}", nonNull);
    Add("nested-choose-1000",
        "{NullableInt:choose(null):{IntValueIfNull:choose(1000|2000):1k|2k}|{:N2}}",
        """{"NullableInt":null,"IntValueIfNull":1000}""");
    Add("nested-choose-2000",
        "{NullableInt:choose(null):{IntValueIfNull:choose(1000|2000):1k|2k}|{:N2}}",
        """{"NullableInt":null,"IntValueIfNull":2000}""");
    Add("nested-split-nine", "{0:choose(1|2):{1:choose(9|8):nine|eight}|other}", "[1,9]");
    Add("nested-split-eight", "{0:choose(1|2):{1:choose(9|8):nine|eight}|other}", "[1,8]");
    Add("nested-split-other", "{0:choose(1|2):{1:choose(9|8):nine|eight}|other}", "[2,9]");
    Add("branch-with-placeholder-1", "{0:choose(1|2):one|{1}two|three}", @"[1,""X""]");
    Add("branch-with-placeholder-2", "{0:choose(1|2):one|{1}two|three}", @"[2,""X""]");
    Add("branch-with-placeholder-3", "{0:choose(1|2):one|{1}two|three}", @"[3,""X""]");

    // Escapes: .NET splits the source text, so an escaped split character still
    // splits and leaves the backslash behind.
    Add("escaped-split-char-left", @"{0:choose(1|2):a\|b|c}", "[1]");
    Add("escaped-split-char-right", @"{0:choose(1|2):a\|b|c}", "[2]");
    Add("escaped-newline", @"{0:choose(1|2):a\nb|c}", "[1]");
    Add("escaped-brace", @"{0:choose(1|2):a\{b|c}", "[1]");
    Add("escaped-colon", @"{0:choose(1|2):a\:b|c}", "[1]");
    Add("escaped-paren-option", @"{0:choose(a\)b|c):one|two|else}", @"[""a)b""]");
    Add("escaped-paren-option-second", @"{0:choose(a\)b|c):one|two|else}", @"[""c""]");
    Add("escaped-paren-option-else", @"{0:choose(a\)b|c):one|two|else}", @"[""x""]");

    // Empty branches and alignment.
    Add("empty-first-branch", "[{0:choose(1|2):|c}]", "[1]");
    Add("empty-second-branch", "[{0:choose(1|2):a|}]", "[2]");
    Add("alignment-right", "[{0,10:choose(1|2):one|two}]", "[1]");
    Add("alignment-left", "[{0,-6:choose(1|2):one|two}]", "[2]");

    // A split character among the hex digits of a `\uXXXX` sequence.
    Add("sliced-unicode-escape", @"{0:choose(1|2):\u00|41}", "[1]");
    Add("sliced-unicode-escape-second", @"{0:choose(1|2):\u00|41}", "[2]");

    // The formatter's own case sensitivity wins over the settings: .NET picks
    // `settings == CaseSensitivity ? settings : CaseSensitivity`, which is
    // always the formatter's own, so this stays case-sensitive.
    Add("case-insensitive-settings", "{0:choose(string|STRING):one|two|default}", @"[""String""]",
        new CaseSettings(CaseSensitivity: CaseSensitivityType.CaseInsensitive));

    // Errors.
    Add("err-no-match", "{0:choose(1|2):1|2}", "[99]");
    Add("err-one-format", "{0:choose(1|2):1}", "[1]");
    Add("err-no-format", "{0:choose(1|2)}", "[1]");
    Add("err-too-few-formats", "{0:choose(1|2|3):1|2}", "[1]");
    Add("err-too-many-formats-one-choice", "{0:choose(1):1|2|3}", "[1]");
    Add("err-too-many-formats", "{0:choose(1|2):1|2|3|4}", "[1]");
    // 3.6.1 dropped the aliases the obsolete `Names` property once held, so
    // no formatter answers to "c".
    Add("err-short-name", "{0:c(1|2):a|b}", "[1]");

    // Error actions that still produce a result.
    Add("err-no-match-ignored", "x{0:choose(1|2):1|2}y", "[99]",
        new CaseSettings(FormatErrorAction: FormatErrorAction.Ignore));
    Add("err-no-match-maintain-tokens", "{0:choose(1|2):1|2}", "[99]",
        new CaseSettings(FormatErrorAction: FormatErrorAction.MaintainTokens));
    // The "fewer than 2 formats" path is a plain FormatException, whose bare
    // message .NET writes into the result — the one choose error whose text we
    // already reproduce.
    Add("err-one-format-in-result", "{0:choose(1|2):single}", "[1]",
        new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult));
}

// ---------------------------------------------------------------------------
// M2: ConditionalFormatter, named "cond".
// ---------------------------------------------------------------------------

static void ConditionalCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("cond-" + id, template, argsJson, settings));

    // Bucket indexing by the floor of the value: one placeholder per argument,
    // all with the same parts.
    const string buckets = "[0,1,2,3,-1,-2]";
    string SixBuckets(string parts) =>
        string.Join(" ", Enumerable.Range(0, 6).Select(i => "{" + i + ":cond:" + parts + "}"));
    Add("buckets-2", SixBuckets("Zero|Other"), buckets);
    Add("buckets-3", SixBuckets("Zero|One|Other"), buckets);
    Add("buckets-4", SixBuckets("Zero|One|Two|Other"), buckets);

    foreach (var (slug, json) in new (string, string)[]
             {
                 ("0_5", "0.5"), ("1_5", "1.5"), ("2_9", "2.9"), ("neg-0_5", "-0.5"), ("neg-zero", "-0.0"),
                 ("almost-one", "0.9999999999999999"), ("2_0", "2.0"),
             })
        Add($"bucket-fraction-{slug}", "{0:cond:Zero|One|Other}", "[" + json + "]");
    Add("bucket-four-parts-2_0", "{0:cond:A|B|C|D}", "[2.0]");

    // Bools, strings, null and objects never reach a third part.
    Add("bool-true", "{0:cond:Yes|No}", "[true]");
    Add("bool-false", "{0:cond:Yes|No}", "[false]");
    Add("bool-three-parts-true", "{0:cond:a|b|c}", "[true]");
    Add("bool-three-parts-false", "{0:cond:a|b|c}", "[false]");
    Add("string-has-value", "{0:cond:{}|Empty}", @"[""Hello""]");
    Add("string-empty", "{0:cond:{}|Empty}", @"[""""]");
    Add("null", "{0:cond:{}|Null}", "[null]");
    Add("string-three-parts-value", "{0:cond:a|b|c}", @"[""x""]");
    Add("string-three-parts-empty", "{0:cond:a|b|c}", @"[""""]");
    Add("string-three-parts-null", "{0:cond:a|b|c}", "[null]");
    Add("object", "{0:cond:Something|Null}", """{"a":1}""");

    // Complex conditions.
    foreach (var (slug, json) in new (string, string)[] { ("0", "0"), ("1", "1"), ("2", "2") })
        Add($"complex-sign-{slug}", "{0:cond:>0?Positive|<0?Negative|=0?Zero}", "[" + json + "]");

    const string ages =
        "{0:cond:<1?Baby|>=1&<4?Toddler|>=4&<=9?Child|=10/=11/=12?Pre-Teen|<18?Teenager|" +
        "<20?Young Adult|<20/<=24&<25?Early Twenties|>55&<100?Senior Citizen|>100?Crazy Old|Adult}";
    foreach (var (slug, json) in new (string, string)[]
             {
                 ("neg-5", "-5"), ("0", "0"), ("0_5", "0.5"), ("1_0", "1.0"), ("1_5", "1.5"),
                 ("5_0", "5.0"), ("11_0", "11.0"), ("14_0", "14.0"), ("18", "18"), ("22", "22"),
                 ("45", "45"), ("60", "60"), ("101", "101"),
             })
        Add($"complex-age-{slug}", ages, "[" + json + "]");

    var comparers = new (string Slug, string Condition, string Json)[]
    {
        ("gt", ">5", "6"), ("ge", ">=6", "6"), ("lt", "<6", "6"), ("le", "<=6", "6"),
        ("eq", "=6", "6"), ("eqeq", "==6", "6"), ("not", "!5", "6"), ("noteq", "!=5", "5"),
    };
    foreach (var (slug, condition, json) in comparers)
        Add($"comparer-{slug}", "{0:cond:" + condition + "?a|b}", "[" + json + "]");

    Add("implicit-and-true", "{0:cond:>1<5?a|b}", "[2]");
    Add("implicit-and-false", "{0:cond:>1<5?a|b}", "[6]");
    Add("first-question-mark-ends-condition", "{0:cond:>0?a?b|c}", "[1]");
    Add("else-branch", "{0:cond:>10?big|else}", "[0]");
    Add("else-before-condition", "{0:cond:>10?a|else|>20?b}", "[5]");
    Add("else-before-condition-match", "{0:cond:>10?a|else|>20?b}", "[15]");
    Add("non-condition-first-part", "{0:cond:else|>10?big}", "[15]");
    Add("empty-first-part", "{0:cond:|>10?a|b}", "[15]");

    // Near-misses: a part that only looks like a condition.
    foreach (var (slug, template, json) in new (string, string, string)[]
             {
                 ("space-after-comparer", "{0:cond:> 5?a|b}", "6"),
                 ("space-before-question", "{0:cond:>5 ?a|b}", "6"),
                 ("plus-sign", "{0:cond:>+5?a|b}", "6"),
                 ("no-value", "{0:cond:>?a|b}", "6"),
                 ("no-comparer", "{0:cond:-5?a|b}", "-5"),
             })
        Add($"near-miss-{slug}", template, "[" + json + "]");

    Add("condition-needs-a-number-string", "{0:cond:>10?a|b|c}", @"[""text""]");
    Add("condition-needs-a-number-bool", "{0:cond:>10?a|b}", "[true]");

    foreach (var (slug, template, json) in new (string, string, string)[]
             {
                 ("leading-dot", "{0:cond:>.5?a|b}", "0.6"),
                 ("trailing-dot", "{0:cond:>5.?a|b}", "6"),
                 ("trailing-sign", "{0:cond:>5.-?a|b}", "-1"),
                 ("leading-minus", "{0:cond:>-5?a|b}", "-1"),
                 ("decimal-condition-int-value", "{0:cond:=1.0?a|b}", "1"),
                 ("int-condition-decimal-value", "{0:cond:=1?a|b}", "1.0"),
                 ("double-rounds-to-decimal", "{0:cond:=0.3?yes|no}", "0.30000000000000004"),
                 ("invariant-parsing", "{0:cond:<=0.25?I am less than 0.25|I am over 0.25}", "0.3"),
                 ("28-digit-condition", "{0:cond:>1.00000000000000000000000000005?a|b}", "1"),
             })
        Add($"condition-value-{slug}", template, "[" + json + "]");

    // Every numeric type reaches the same comparison.
    foreach (var (slug, json) in new (string, string)[]
             {
                 ("int", "123"), ("double", "123.0"), ("i32", """{"$i32":"123"}"""),
             })
        Add($"numeric-type-{slug}", "{0:cond:=123?yes|no}", "[" + json + "]");

    // A changed split character, nesting and escapes.
    Add("nested-placeholder-zero", "{0:cond:{1}|c}", @"[0,""N""]");
    Add("nested-placeholder-one", "{0:cond:{1}|c}", @"[1,""N""]");
    Add("nested-value-in-condition", "{0:cond:>0?[{}]|c}", "[1]");
    Add("escaped-split-char", @"{0:cond:a\|b|c}", "[0]");
    // A split character among the hex digits of a `\uXXXX` sequence: each
    // piece resolves what is left of the sequence, so `\u00` is a NUL.
    Add("sliced-unicode-escape", @"{0:cond:\u00|41|c}", "[0]");
    Add("sliced-unicode-escape-second", @"{0:cond:\u00|41|c}", "[1]");
    Add("sliced-unicode-escape-last", @"{0:cond:z|\u00|41}", "[1]");

    // Alignment, as `choose` has.
    Add("alignment-right", "[{0,10:cond:a|b}]", "[0]");
    Add("alignment-left", "[{0,-10:cond:a|b}]", "[1]");

    // A 64-bit unsigned value: the bucket index casts to Int32 and overflows,
    // but a condition is compared as a decimal and returns before the cast.
    Add("ulong-max-overflow", "{0:cond:a|b}", JsonUlongMax());
    Add("ulong-max-condition", "{0:cond:>1?a|b}", JsonUlongMax());

    // Errors.
    Add("err-one-part", "{0:cond:Yes}", "[1]");
    Add("err-empty-format", "{0:cond:}", "[1]");
    Add("err-all-conditions-fail", "{0:cond:>10?big|>20?huge}", "[0]");
    Add("err-int32-overflow", "{0:cond:a|b}", "[3000000000]");
    Add("err-int32-overflow-negative", "{0:cond:a|b}", "[-3000000000]");
    Add("err-int32-max", "{0:cond:a|b}", "[2147483647]");
    Add("err-decimal-overflow", "{0:cond:a|b}", "[1e30]");
    Add("err-decimal-nan", "{0:cond:a|b}", """[{"$f":"NaN"}]""");
    Add("err-decimal-infinity", "{0:cond:a|b}", """[{"$f":"Infinity"}]""");
    Add("err-decimal-nan-with-condition", "{0:cond:>1?a|b}", """[{"$f":"NaN"}]""");
    Add("err-condition-overflow-value", "{0:cond:>1?a|b}", "[3000000000]");
    foreach (var (slug, condition) in new[]
             {
                 ("two-dots", ">5.5.5"), ("lone-minus", ">-"), ("lone-dot", ">."), ("inner-minus", ">5-5"),
             })
        Add($"err-condition-format-{slug}", "{0:cond:" + condition + "?a|b}", "[6]");
    Add("err-condition-too-large", "{0:cond:>99999999999999999999999999999999?a|b}", "[6]");
    Add("err-long-name", "{0:conditional:a|b}", "[1]");
}

// ---------------------------------------------------------------------------
// Auto-detection: which of the two `|`-splitting formatters claims an unnamed
// format, and what the other one would have said.
// ---------------------------------------------------------------------------

static void AutoDetectCases(List<GoldenCase> cases)
{
    // PluralLocalizationFormatter is consulted first, so a number takes its
    // rule and not the conditional's bucket index. The values are the ones the
    // two disagree on: 0 buckets to "a" but pluralizes to "other", and Russian
    // sends 22 to its "few" arm where the bucket index would be past the end.
    foreach (var (culture, slug) in new[] { ("en-US", "en"), ("ru", "ru"), ("fr-FR", "fr"), ("ja", "ja") })
    foreach (var (countSlug, json) in new[] { ("0", "0"), ("1", "1"), ("2", "2"), ("22", "22") })
        cases.Add(new GoldenCase(
            $"autodetect-number-{slug}-{countSlug}", "{0:a|b|c}", "[" + json + "]", Culture: culture));

    // A value the plural formatter cannot take falls through to the
    // conditional one.
    cases.Add(new GoldenCase("autodetect-bool-true", "{0:part(s)|car}", "[true]", Culture: "en-US"));
    cases.Add(new GoldenCase("autodetect-bool-false", "{0:part(s)|car}", "[false]", Culture: "en-US"));
    cases.Add(new GoldenCase("autodetect-string", "{0:has|empty}", @"[""x""]", Culture: "en-US"));
    cases.Add(new GoldenCase("autodetect-string-empty", "{0:has|empty}", @"[""""]", Culture: "en-US"));
    cases.Add(new GoldenCase("autodetect-null", "{0:has|null}", "[null]", Culture: "en-US"));
    cases.Add(new GoldenCase("autodetect-empty-name", "{0::a|b}", "[1]", Culture: "en-US"));
    // One part is not enough to auto-detect either formatter.
    cases.Add(new GoldenCase("autodetect-single-part", "{0:zero}", "[0]", Culture: "en-US"));

    // ListFormatter sorts ahead of both and auto-detects as well, so on a list
    // .NET renders the items with the first part and the second as the spacer.
    // It lands in M3; until then the Rust runner skips this case.
    cases.Add(new GoldenCase(
        "autodetect-list", "{0:one|many}", @"[[""x"",""y""]]", Culture: "en-US"));
}

// ---------------------------------------------------------------------------
// Culture data, end to end: the generated table in
// `crates/smartformat/src/fmt/culture` against the .NET it was generated from.
// ---------------------------------------------------------------------------

/// Every culture the generated table carries, minus the invariant one.
static string[] GeneratedCultures() =>
[
    "ar", "ar-SA", "cs", "da", "de", "de-AT", "de-CH", "de-DE", "en", "en-GB", "en-US",
    "es", "es-ES", "es-MX", "fi", "fr", "fr-FR", "is", "is-IS", "it", "ja", "ko", "nb",
    "nl", "pl", "pt", "pt-BR", "pt-PT", "ru", "sv", "tr", "uk", "zh-CN", "zh-Hans",
];

static string CultureSlug(string culture) => culture.ToLowerInvariant();

static void CultureNumberCases(List<GoldenCase> cases)
{
    const string big = "-1234567.891";
    const string bigPositive = "1234567.891";

    // Four specifiers over every culture: the group and decimal separators and
    // the negative sign (N), both arms of the 17-entry currency pattern table
    // (C), and the percent pattern (P1).
    var specs = new (string Slug, string Spec, string Value)[]
    {
        ("N", "N", big),
        ("C-neg", "C", big),
        ("C-pos", "C", bigPositive),
        ("P1", "P1", big),
    };
    foreach (var culture in GeneratedCultures())
    foreach (var (slug, spec, value) in specs)
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-{slug}",
            "{0:" + spec + "}",
            "[" + value + "]",
            Culture: culture));

    // The default number of decimal digits is culture data too, and is 3 for
    // most ICU cultures rather than the invariant 2.
    foreach (var culture in new[] { "en-US", "de-DE", "fr-FR", "is-IS", "ja", "ar-SA" })
    {
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-N-default-digits", "{0:N}", "[1234.5]", Culture: culture));
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-N2", "{0:N2}", "[1234.5]", Culture: culture));
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-P-default-digits", "{0:P}", "[0.1234]", Culture: culture));
    }

    // The negative sign is not always a hyphen.
    foreach (var culture in new[] { "sv", "fi", "nb", "is-IS", "tr", "ar-SA", "de-CH" })
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-N0-neg", "{0:N0}", "[-42]", Culture: culture));

    // An explicit precision beats the culture's CurrencyDecimalDigits.
    cases.Add(new GoldenCase("culture-num-is-is-C3", "{0:C3}", "[" + big + "]", Culture: "is-IS"));
    cases.Add(new GoldenCase("culture-num-ja-C0", "{0:C0}", "[" + big + "]", Culture: "ja"));

    // Exponent notation carries the culture's positive sign.
    foreach (var culture in new[] { "ar-SA", "de-DE", "sv" })
    {
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-E2-pos", "{0:E2}", "[" + bigPositive + "]", Culture: culture));
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-E2-neg", "{0:E2}", "[" + big + "]", Culture: culture));
    }

    // NaN and the infinities are culture data as well.
    foreach (var culture in new[] { "de-DE", "sv", "ru", "ar-SA", "ja" })
    foreach (var (slug, json) in new (string, string)[]
             {
                 ("nan", """{"$f":"NaN"}"""), ("inf", """{"$f":"Infinity"}"""),
                 ("neg-inf", """{"$f":"-Infinity"}"""),
             })
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-{slug}", "{0}", "[" + json + "]", Culture: culture));

    // A plain integer and a plain double still go through the culture.
    foreach (var culture in new[] { "de-DE", "fr-FR", "ru", "ar-SA", "tr" })
    {
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-plain-double", "{0}", "[-1234.5]", Culture: culture));
        cases.Add(new GoldenCase(
            $"culture-num-{CultureSlug(culture)}-plain-int", "{0}", "[-1234567]", Culture: culture));
    }
}

static void CultureDateCases(List<GoldenCase> cases)
{
    // A Monday afternoon, and a Tuesday morning for the AM designator and for
    // the genitive month names a day-first pattern selects.
    const string afternoon = """[{"$dt":"2009-06-15T13:45:30.0000000"}]""";
    const string morning = """[{"$dt":"2024-03-05T09:07:03.0000000"}]""";

    var specs = new (string Slug, string Spec)[]
    {
        ("d-lc", "d"), ("D", "D"), ("t-lc", "t"), ("T", "T"), ("f-lc", "f"),
    };
    foreach (var culture in GeneratedCultures())
    foreach (var (slug, spec) in specs)
        cases.Add(new GoldenCase(
            $"culture-date-{CultureSlug(culture)}-{slug}",
            "{0:" + spec + "}",
            afternoon,
            Culture: culture));

    // The morning date, whose day number is 5: a one-digit day, a different
    // month, and the AM designator.
    foreach (var culture in GeneratedCultures())
    foreach (var (slug, spec) in new (string, string)[] { ("D", "D"), ("t-lc", "t"), ("y-lc", "y") })
        cases.Add(new GoldenCase(
            $"culture-date2-{CultureSlug(culture)}-{slug}",
            "{0:" + spec + "}",
            morning,
            Culture: culture));

    // The remaining standard specifiers over the cultures whose patterns
    // differ most.
    foreach (var culture in new[] { "ru", "uk", "cs", "pl", "fi", "ar", "ar-SA", "ja", "ko", "zh-CN", "tr", "es-MX", "de-CH", "en-GB" })
    foreach (var (slug, spec) in new (string, string)[]
             {
                 ("F", "F"), ("g-lc", "g"), ("G", "G"), ("M", "M"), ("none", ""),
             })
        cases.Add(new GoldenCase(
            $"culture-date-{CultureSlug(culture)}-{slug}",
            spec.Length == 0 ? "{0}" : "{0:" + spec + "}",
            afternoon,
            Culture: culture));

    // The culture-invariant specifiers stay invariant whatever the culture.
    foreach (var culture in new[] { "ru", "ar-SA", "ja", "de-DE" })
    foreach (var (slug, spec) in new (string, string)[] { ("O", "O"), ("R", "R"), ("s-lc", "s"), ("u-lc", "u") })
        cases.Add(new GoldenCase(
            $"culture-date-{CultureSlug(culture)}-{slug}",
            "{0:" + spec + "}",
            afternoon,
            Culture: culture));
}

// ---------------------------------------------------------------------------
// How a culture *name* resolves. .NET reads the text after the single `_` of a
// name as an alternate sort order, not as a subtag, and a sort order changes
// how strings compare and nothing about how values format: `en_US` carries the
// data of the neutral culture `en`, not of `en-US`. What sits before the `_`
// wins when it is itself a full culture (`de-DE_phoneb`).
// ---------------------------------------------------------------------------

static void CultureNameCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, string culture) =>
        cases.Add(new GoldenCase("culture-name-" + id, template, argsJson, Culture: culture));

    // The load-bearing one: `en`'s currency symbol is `¤`, `en-US`'s is `$`.
    Add("alt-sort-currency", "{0:C2}", "[1234.5]", "en_US");
    Add("alt-sort-currency-de", "{0:C2}", "[1234.5]", "de_DE");
    // Here the part before the `_` is a full culture, and it is kept.
    Add("alt-sort-base-is-a-culture", "{0:C2}", "[1234.5]", "de-DE_phoneb");
    // The DateTimeFormat half of the same lookup.
    Add("alt-sort-date", "{0:D}", """[{"$dt":"2009-06-15T13:45:30.0000000"}]""", "en_US");
    // One underscore, so `us-posix` is all sort order and the culture is `en`.
    Add("alt-sort-with-subtags", "{0:C2}", "[1234.5]", "en_US-POSIX");
    // The name is matched case-insensitively before the sort order is dropped.
    Add("alt-sort-uppercase", "{0:C2}", "[1234.5]", "EN_us");
}

// ---------------------------------------------------------------------------
// Where the M2 formatters meet the culture: what a nested placeholder renders
// as, and what the option comparison does with a culture that has one.
// ---------------------------------------------------------------------------

static void CultureFormatterCases(List<GoldenCase> cases)
{
    // A nested placeholder inside a branch renders with the culture of the
    // call, not the invariant one.
    cases.Add(new GoldenCase(
        "culture-fmt-choose-nested-n2", "{0:choose(1|2):{1:N2}|other}", "[1,1234.5]", Culture: "de-DE"));
    cases.Add(new GoldenCase(
        "culture-fmt-cond-nested-value", "{0:cond:>0?[{}]|c}", "[1234.5]", Culture: "de-DE"));
    cases.Add(new GoldenCase(
        "culture-fmt-plural-nested-value", "{0:plural:{} item|{} items}", "[1234.5]", Culture: "fr-FR"));
    cases.Add(new GoldenCase(
        "culture-fmt-plural-nested-date", "{0:plural:one|other} {1:D}",
        """[2,{"$dt":"2009-06-15T13:45:30.0000000"}]""", Culture: "ru"));

    // Condition values are parsed with the invariant culture whatever the
    // culture of the call, so a comma-decimal culture does not change which
    // branch a value takes.
    cases.Add(new GoldenCase(
        "culture-fmt-cond-invariant-condition",
        "{0:cond:<=0.25?below|above}", "[0.3]", Culture: "de-DE"));
    cases.Add(new GoldenCase(
        "culture-fmt-cond-invariant-condition-below",
        "{0:cond:<=0.25?below|above}", "[0.2]", Culture: "de-DE"));

    // Choose compares the value's own string form against the options. .NET
    // renders it with the *thread* culture, so only a value whose ToString is
    // the same under every culture belongs here: strings, bools, null, and
    // non-negative integers — a *negative* integer is not one of them, because
    // the negative sign is culture data (sv, fi and nb use U+2212).
    cases.Add(new GoldenCase(
        "culture-fmt-choose-int", "{0:choose(1|2):eins|zwei|sonst}", "[2]", Culture: "de-DE"));
    cases.Add(new GoldenCase(
        "culture-fmt-choose-bool", "{0:choose(True|False):ja|nein}", "[true]", Culture: "de-DE"));

    // Case folding under a culture whose casing rules differ from the
    // invariant one; the formatter's own comparison is ordinal.
    cases.Add(new GoldenCase(
        "culture-fmt-choose-umlaut", "{0:choose(Ä|B):a-umlaut|b|else}", "[\"Ä\"]", Culture: "de-DE"));
    cases.Add(new GoldenCase(
        "culture-fmt-choose-umlaut-lower", "{0:choose(Ä|B):a-umlaut|b|else}", "[\"ä\"]", Culture: "de-DE"));
    cases.Add(new GoldenCase(
        "culture-fmt-choose-dotted-i", "{0:choose(I|i):upper|lower|else}", "[\"i\"]", Culture: "tr"));

    // .NET's option comparison is culture-aware, so a character the collation
    // ignores compares equal; ours is ordinal. Both pinned as divergences.
    cases.Add(new GoldenCase(
        "culture-fmt-choose-soft-hyphen-value", "{0:choose(ab):match|else}", "[\"a­b\"]"));
    cases.Add(new GoldenCase(
        "culture-fmt-choose-soft-hyphen-option", "{0:choose(­):match|else}", "[\"\"]"));
}

// ---------------------------------------------------------------------------
// The exact text of an M2 formatter's error. A case that throws only records
// the exception type, so the message is only observable through
// FormatErrorAction.OutputErrorInResult, which writes it into the result.
//
// .NET writes two different things there: a FormattingException's own Message,
// which quotes the template and points a caret at the failure, and the bare
// Message of any other exception the evaluator wraps.
// ---------------------------------------------------------------------------

static void FormatterErrorTextCases(List<GoldenCase> cases)
{
    var output = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);
    var maintain = new CaseSettings(FormatErrorAction: FormatErrorAction.MaintainTokens);

    void Add(string id, string template, string argsJson, string culture = "") =>
        cases.Add(new GoldenCase("errtext-" + id, template, argsJson, output, culture));

    // ChooseFormatter.
    Add("choose-no-match", "{0:choose(1|2):1|2}", "[99]");
    Add("choose-no-match-offset", "prefix {0:choose(1|2):1|2}", "[99]");
    Add("choose-too-few-formats", "{0:choose(1|2|3):1|2}", "[1]");
    Add("choose-too-many-formats", "{0:choose(1):1|2|3}", "[1]");
    Add("choose-no-format", "{0:choose(1|2)}", "[1]");
    // A prefix outside the BMP: the index and the caret line count UTF-16 code
    // units, so the emoji counts twice.
    Add("choose-no-match-astral-offset", "\U0001F600{0:choose(1|2):1|2}", "[99]");
    Add("choose-unknown-name", "{0:c(1|2):a|b}", "[1]");
    Add("choose-one-format", "{0:choose(1|2):single}", "[1]");

    // ConditionalFormatter.
    Add("cond-one-part", "{0:cond:Yes}", "[1]");
    Add("cond-empty-format", "{0:cond:}", "[1]");
    Add("cond-all-conditions-fail", "{0:cond:>10?big|>20?huge}", "[0]");
    Add("cond-int32-overflow", "{0:cond:a|b}", "[3000000000]");
    Add("cond-decimal-overflow", "{0:cond:a|b}", "[1e30]");
    Add("cond-bad-condition-value", "{0:cond:>5.5.5?a|b}", "[6]");
    Add("cond-condition-too-large", "{0:cond:>99999999999999999999999999999999?a|b}", "[6]");
    Add("cond-unknown-name", "{0:conditional:a|b}", "[1]");
    Add("cond-ulong-overflow", "{0:cond:a|b}", JsonUlongMax());

    // PluralLocalizationFormatter.
    Add("plural-one-word", "{0:plural:One}", "[1]", "en-US");
    Add("plural-five-words", "{0:plural:a|b|c|d|e}", "[1]", "en-US");
    Add("plural-five-words-offset", "prefix {0:plural:a|b|c|d|e}", "[1]", "en-US");
    Add("plural-string-value", "{0:plural:One|Two}", @"[""1234""]", "en-US");
    Add("plural-null-value", "{0:plural:One|Two}", "[null]", "en-US");
    Add("plural-bool-value", "{0:plural:One|Two}", "[false]", "en-US");
    Add("plural-unknown-language", "{0:plural(xx):a|b}", "[1]", "en-US");
    // The text of the CultureNotFoundException the runtime throws. It is the
    // one golden whose wording belongs to .NET itself rather than to
    // SmartFormat, so a runtime that rewords it moves this case.
    Add("plural-invalid-culture-name", "{0:plural(en-):a|b}", "[1]", "en-US");
    Add("plural-invalid-culture-name-upper", "{0:plural(EN--US):a|b}", "[1]", "en-US");
    Add("plural-unknown-name", "{0:p:one|many}", "[2]", "en-US");

    // A placeholder that names a formatter nothing answers to. .NET passes
    // `Selector?.SelectorIndex ?? -1` and turns -1 into the *format*'s start
    // offset, so a placeholder with no selector reports a different index from
    // one with a selector — which is exactly what a nameless `{:t:...}`
    // produces when the template formatter is not registered.
    Add("no-formatter-no-selector", "{:nope:x}", "[42]");
    Add("no-formatter-no-selector-options", "{:nope()}", "[42]");
    Add("no-formatter-positional-selector", "{0:nope:x}", "[42]");
    Add("no-formatter-named-selector", "{Name:nope:x}", """{"Name":"Alice"}""");
    Add("no-formatter-alignment-only", "{,5:nope:x}", "[42]");

    // The same failures under MaintainTokens, which reconstructs the
    // placeholder from the template instead of reporting anything.
    cases.Add(new GoldenCase(
        "errtext-maintain-choose-no-match", "{0:choose(1|2):1|2}", "[99]", maintain));
    cases.Add(new GoldenCase(
        "errtext-maintain-cond-one-part", "{0:cond:Yes}", "[1]", maintain));
    cases.Add(new GoldenCase(
        "errtext-maintain-plural-one-word", "{0:plural:One}", "[1]", maintain, "en-US"));
}

// ---------------------------------------------------------------------------
// M3: ListFormatter, named "list".
//
// The formatter sorts first of all, so it also decides what an unnamed
// `|`-separated format means when the value is a collection. Its format is
// split into at most five parts — item, spacer, last spacer, two-item spacer —
// with a bound of four separators that is observable through a fifth part the
// split never reaches.
// ---------------------------------------------------------------------------

static void ListFormatterCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("list-" + id, template, argsJson, settings));

    const string abc = """[["a","b","c"]]""";
    const string ab = """[["a","b"]]""";
    const string a = """[["a"]]""";
    const string none = "[[]]";
    const string oneToFive = "[[1,2,3,4,5]]";
    const string atoE = """[["A","B","C","D","E"]]""";
    const string twoLists = """[["A","B","C","D","E"],["One","Two","Three","Four","Five"]]""";

    // -- Shape: how many parts the format has decides where each spacer goes.
    Add("two-part", "{0:list:{}|, }", abc);
    Add("two-part-two-items", "{0:list:{}|, }", ab);
    Add("three-part", "{0:list:{}|, |, and }", abc);
    Add("three-part-two-items", "{0:list:{}|, |, and }", ab);
    Add("four-part", "{0:list:{}|, |, and | & }", abc);
    Add("four-part-two-items", "{0:list:{}|, |, and | & }", ab);
    Add("four-part-one-item", "{0:list:{}|, |, and | & }", a);
    Add("five-parts", "{0:list:{}|1|2|3|4}", abc);
    Add("empty-list", ">{0:list:{}|, |, and }<", none);
    Add("empty-list-two-part", ">{0:list:{}|, }<", none);
    Add("empty-spacer", "{0:list:{}|}", abc);
    Add("no-item-format", "{0:list:|}", oneToFive);
    Add("no-item-format-comma-spacer", "{0:list:|,}", oneToFive);
    Add("item-spec-n2", "{0:list:N2|, |, and }", oneToFive);
    Add("item-suffix", "{0:list:{}-|}", atoE);
    Add("last-spacer", "{0:list:{}|-|+}", atoE);
    Add("item-in-parens", "{0:list:({})|, |, and }", atoE);

    // -- Auto-detection: ListFormatter sorts ahead of plural and cond, so an
    // unnamed `|` format over a collection is a list.
    Add("autodetect-two-part", "{0:one|many}", abc);
    Add("autodetect-three-part", "{0:one|many|last}", abc);
    Add("autodetect-two-items", "{0:one|many|last}", ab);

    // -- The {Index} selector, which the list source answers while iterating.
    Add("index-in-item", "{0:list:{} = {Index}|, }", atoE);
    Add("index-other-list-dotted", "{0:list:{} = {1.Index}|, }", twoLists);
    Add("index-other-list-bracket", "{0:list:{} = {1[Index]}|, }", twoLists);
    Add("index-other-list-uppercase", "{0:list:{} = {1.INDEX}|, }", twoLists);
    // A second list that runs out: the out-of-range item falls back to the
    // list being iterated, which is what the parent scope holds.
    Add("index-shorter-second-list", "{0:list:{}={1.Index}|, }", """[[1,2,3],["x"]]""");
    // Outside any iteration the index is -1 for a value the list source
    // answers for at all, and an unhandled selector for anything else.
    Add("index-outside-list", "{Index}", "[[1,2,3]]");
    Add("index-outside-string", "{Index}", @"[""abc""]");
    Add("index-outside-map", "{Index}", """{"a":1}""");
    Add("index-outside-int", "{Index}", "[42]");
    Add("index-as-member", "{Items.Index}", """{"Items":[1,2]}""");
    Add("index-after-list", "{0:list:{}|,}-{Index}", abc);
    Add("index-nested", "{0:list:{Index}: {:list:{} = {Index}|, }|; }",
        """[[["O","n","e"],["T","w","o"]]]""");
    Add("index-restored-after-nested", "{0:list:[{Index}:{1:list:{}|,}:{Index}]|;}",
        """[[1,2,3],["x","y"]]""");

    // -- What a spacer is formatted against: the outermost value, not the item.
    const string names = """{"Names":["John","Mary","Amy"],"Split":", ","IsAnd":true}""";
    const string namesNor = """{"Names":["John","Mary","Amy"],"Split":", ","IsAnd":false}""";
    Add("spacer-from-map", "{Names:list:{}|{Split}| {IsAnd:and|nor} }", names);
    Add("spacer-from-map-false", "{Names:list:{}|{Split}| {IsAnd:and|nor} }", namesNor);
    Add("spacer-positional", "{0:list:{}|{1}| {2} }", """[["John","Mary","Amy"],", ","and"]""");
    Add("item-reads-outer-map", "{Names:list:{}{Split}|, }",
        """{"Names":["John","Mary"],"Split":"+"}""");

    // -- Alignment applies per item; a literal spacer is not aligned, but a
    // placeholder inside a spacer keeps the alignment the parser gave it.
    Add("alignment-right", ">{0,5:list:{}|, }<", abc);
    Add("alignment-left", ">{0,-5:list:{}|, }<", abc);
    Add("alignment-with-spec", ">{0,5:list:N2|, }<", "[[1,2]]");
    Add("alignment-placeholder-spacer", ">{0,5:list:{}|{1}}<", """[["a","b","c"],"+"]""");

    // -- The split stops after four separators, so a fifth part is never cut
    // and a crossed literal in it is never resolved.
    Add("split-limit-fifth-part", @"{0:list:{}|-|+|*|x\u12}", abc);
    Add("split-limit-fourth-part", @"{0:list:{}|-|+|x\u12|z}", abc);
    Add("split-crossed-spacer", @"{0:list:{}|x\u12}", abc);
    Add("spacer-escaped-newline", @"{0:list:{}|\n}", ab);

    // -- Errors. The formatter needs a collection and at least two parts.
    Add("err-not-a-list", "{0:list:{}|, |, and }", @"[""not a list""]");
    Add("err-int", "{0:list:{}|, |, and }", "[42]");
    Add("err-null", "{0:list:{}|, |, and }", "[null]");
    Add("err-one-part", "{0:list:{}}", abc);
    Add("nullable-null", "{TheList?:list:{}|, |, and }", """{"TheList":null}""");
    Add("err-short-name", "{0:l:{}|,}", abc);
    Add("err-capitalized-name", "{0:List:{}|,}", abc);
    cases.Add(new GoldenCase(
        "list-err-not-a-list-in-result", "{0:list:{}|, |, and }", @"[""not a list""]",
        new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult)));
    cases.Add(new GoldenCase(
        "list-err-one-part-in-result", "{0:list:{}}", abc,
        new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult)));

    // -- Where a list meets the other formatters.
    // The invariant culture pluralizes as English, as `plural-invariant-*` pins.
    Add("item-runs-plural", "{0:list:{} {:plural:item|items}|, }", "[[1,2]]");
    Add("item-runs-cond", "{0:list:{:cond:zero|one|more}|, }", "[[0,1,2]]");
    Add("item-runs-substr", "{0:list:{:substr(0,2)}|, }", @"[[""alpha"",""bravo""]]");
    Add("item-runs-isnull", "{0:list:{:isnull:-|{}}|,}", @"[[""a"",null,""b""]]");
    Add("inside-isnull", "{0:isnull:none|{:list:{}|-}}", abc);
    Add("inside-choose", "{0:choose(1|2):{1:list:{}|-}|other}", """[1,["a","b"]]""");

    // -- More shapes of the same three moving parts.
    Add("one-item-two-part", "{0:list:{}|, }", a);
    Add("one-item-three-part", "{0:list:{}|, |, and }", a);
    Add("null-items", "{0:list:[{}]|, }", """[[null,"b",null]]""");
    Add("index-in-spacer", "{0:list:{}|[{Index}]}", abc);
    Add("index-in-empty-list", "{0:list:{Index}|,}", none);
    Add("spacer-with-spec", "{0:list:{}|{1:N2}}", """[["a","b"],2.5]""");
    Add("deeply-nested", "{0:list:[{:list:({:list:{}|.})|-}]|;}", "[[[[1,2],[3]],[[4]]]]");
    Add("of-lists-aligned", ">{0,4:list:{:list:{}|-}|;}<", "[[[1,2],[3,4]]]");
    Add("index-in-nested-spacer", "{0:list:{}|{1:list:{}|+}{Index}}", """[["a","b","c"],["x","y"]]""");
    Add("named-cond-on-list", "{0:cond:full|empty}", abc);
    Add("named-choose-on-list", "{0:choose(1|2):a|b|else}", abc);

    // -- Lists of maps and lists of lists.
    const string people = """
        {"People":[
            {"FirstName":"Jim","Friends":[{"FirstName":"Dwight"},{"FirstName":"Michael"}]},
            {"FirstName":"Pam","Friends":[{"FirstName":"Dwight"},{"FirstName":"Michael"}]},
            {"FirstName":"Dwight","Friends":[{"FirstName":"Michael"}]}]}
        """;
    Add("of-maps", "{People:list:{:{FirstName}}|, }", people);
    Add("of-maps-nested",
        "{People:list:{:{FirstName}'s friends: {Friends:list:{FirstName}|, }}|; }", people);
    Add("of-lists", @"{0:list:{:list:{:D3}|, |, }|\n|\n}", "[[[1,2,3],[4,5,6],[7,8,9]]]");
    Add("of-lists-index", "{0:list:{Index}={:list:{}|+}|;}", "[[[1,2],[3,4]]]");

    // -- The two knobs on the formatter itself.
    var tildeSplit = new CaseSettings(ListSplitChar: '~');
    Add("splitchar-tilde", "{0:list:{}~, ~, and }", abc, tildeSplit);
    // The pipe is an ordinary character once the split char has moved.
    Add("splitchar-tilde-pipe-is-literal", "{0:list:{}|x~, }", abc, tildeSplit);
    var noAutoDetect = new CaseSettings(ListCanAutoDetect: false);
    // With auto-detection off the `|` format falls to the next formatter that
    // takes it, which for a collection is the plural formatter.
    Add("autodetect-off", "{0:one|many}", abc, noAutoDetect);
    // Naming the formatter still works: CanAutoDetect only governs the
    // unnamed path.
    Add("autodetect-off-named", "{0:list:{}|, }", abc, noAutoDetect);

    // -- Divergences, held here with .NET's answer and skipped by the runner.
    // A .NET Dictionary is IEnumerable, so it formats as a list of its pairs.
    Add("map-is-enumerable", "{0:list:{}|, }", """[{"a":1,"b":2}]""");
    // No format at all: the formatter declines, and DefaultFormatter renders
    // the collection's CLR type name.
    Add("no-format", "{0:list}", abc);
    // A custom numeric pattern as the item format: the port's documented
    // non-goal, where `D2` above is the standard-specifier equivalent.
    Add("item-custom-pattern", "{0:list:00|, }", "[[1,2]]");
}

// ---------------------------------------------------------------------------
// M3: SubStringFormatter, named "substr".
//
// Counted in UTF-16 code units, which is what makes the astral cases the
// compatibility trap: a cut can split a surrogate pair.
// ---------------------------------------------------------------------------

static void SubStringCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("substr-" + id, template, argsJson, settings));

    var longJohn = JsonString("Long John");
    const string nul = "[null]";
    const string number = "[12345]";
    var astral = JsonString("\U0001F600abc");

    // -- Start and length, in both signs.
    foreach (var (slug, options) in new (string, string)[]
             {
                 // Not a formatter name at all: without a `(` or a second `:`
                 // the parser leaves `substr` in the format, where a string
                 // ignores it.
                 ("name-is-a-format-spec", ""),
                 ("start-0", "(0)"),
                 ("start-5", "(5)"),
                 ("start-neg4", "(-4)"),
                 ("start-neg4-length-2", "(-4,2)"),
                 ("spaced-options", "(-4, 2)"),
                 ("spaces-around-options", "( -4 , 2 )"),
                 ("start-neg4-length-neg1", "(-4,-1)"),
                 ("explicit-plus", "(+1)"),
                 ("leading-zeros", "(0000000005)"),
                 ("three-options", "(1,2,3)"),
                 ("start-at-end", "(9)"),
                 ("start-at-end-length-0", "(9,0)"),
                 ("length-neg9", "(0,-9)"),
             })
        Add(slug, "{0:substr" + options + "}", longJohn);

    // -- Out of range, under each of the three behaviors. Only `start + length`
    // past the end is governed by the setting; a start index still negative
    // after counting from the end, and a length still negative after counting
    // back, are out of range under all three.
    var outOfRange = new (string Slug, string Options)[]
    {
        ("start-past-end", "(999)"),
        ("start-past-end-with-length", "(999,1)"),
        ("length-past-end", "(0,999)"),
        ("start-3-length-7", "(3,7)"),
        ("start-3-length-6", "(3,6)"),
        ("start-9-length-1", "(9,1)"),
        ("start-neg999", "(-999)"),
        ("start-neg999-length-3", "(-999,3)"),
        ("length-neg999", "(0,-999)"),
        ("start-5-length-neg9", "(5,-9)"),
        ("start-neg4-length-neg5", "(-4,-5)"),
        ("length-int-min", "(0,-2147483648)"),
        ("int-max-both", "(2147483647,2147483647)"),
    };
    foreach (var behavior in new[]
             {
                 SubStringFormatter.SubStringOutOfRangeBehavior.ReturnEmptyString,
                 SubStringFormatter.SubStringOutOfRangeBehavior.ReturnStartIndexToEndOfString,
                 SubStringFormatter.SubStringOutOfRangeBehavior.ThrowException,
             })
    foreach (var (slug, options) in outOfRange)
        cases.Add(new GoldenCase(
            $"substr-oor-{behavior switch
            {
                SubStringFormatter.SubStringOutOfRangeBehavior.ReturnEmptyString => "empty",
                SubStringFormatter.SubStringOutOfRangeBehavior.ReturnStartIndexToEndOfString => "toend",
                _ => "throw",
            }}-{slug}",
            "{0:substr" + options + "}", longJohn,
            new CaseSettings(SubStringOutOfRangeBehavior: behavior)));

    // -- Options that are not an integer at all, and integers an Int32 cannot
    // hold. The message quotes the option exactly as it was written.
    foreach (var (slug, options) in new (string, string)[]
             {
                 ("empty", "()"),
                 ("comma-only", "(,)"),
                 ("letter", "( x )"),
                 ("blank", "( )"),
                 ("blank-then-length", "(  ,2)"),
                 ("two-letters", "(x,y)"),
                 ("length-letter", "(0,y)"),
                 ("space-separated", "(1 2)"),
                 ("decimal", "(1.0)"),
                 ("hex-prefix", "(0x1)"),
                 ("arabic-indic-digit", "(\u0663)"),
                 ("pipe-separated", "(1|2)"),
                 ("int32-overflow", "(2147483648)"),
                 ("int32-underflow", "(-2147483649)"),
                 ("length-overflow", "(0,99999999999)"),
             })
        Add("err-option-" + slug, "{0:substr" + options + "}", longJohn);

    // A value that is not a string at all.
    Add("err-non-string", "{0:substr(0,2)}", number);
    Add("err-bool", "{0:substr(0,2)}", "[true]");
    Add("err-list", "{0:substr(0,2)}", """[[1,2]]""");

    // -- The format after the options must be nested placeholders only.
    Add("err-format-is-text", "{0:substr(0,2):just text}", longJohn);
    Add("format-empty", "{0:substr(0,2):}", longJohn);
    Add("format-self", "{0:substr(0,2):{}}", longJohn);
    Add("format-brackets", "{0:substr(0,4):[{}]}", longJohn);
    Add("format-nested-substr", "{0:substr(0,4):{:substr(1,2)}}", longJohn);
    Add("alignment-right", "[{0,15:substr(0,4)}]", longJohn);
    Add("alignment-left", "[{0,-15:substr(0,4)}]", longJohn);
    Add("alignment-with-format", "[{0,10:substr(0,4):[{}]}]", longJohn);

    // -- A null value short-circuits before the options are even parsed.
    Add("null", "{0:substr(0,3)}", nul);
    Add("null-aligned", "[{0,10:substr(0,3)}]", nul);
    Add("null-bad-options", "{0:substr(oops)}", nul);
    Add("null-nested-isnull", "{0:substr(0,3):{:isnull:It is null}}", nul);
    Add("null-format-is-text", "{0:substr(0,3):plain}", nul);

    // -- Surrogate pairs: the counting is in UTF-16 code units, so a cut can
    // split one and leave a half behind.
    Add("astral-first-half", "{0:substr(0,1)}", astral);
    Add("astral-whole-pair", "{0:substr(0,2)}", astral);
    Add("astral-second-half-onwards", "{0:substr(1)}", astral);
    Add("astral-after-pair", "{0:substr(2)}", astral);
    Add("astral-from-end", "{0:substr(-3)}", astral);
    Add("astral-length-past-end", "{0:substr(0,6)}", astral);
    Add("astral-whole", "{0:substr(0,5)}", astral);

    // -- A few more edges of the same arithmetic.
    Add("zero-length", "{0:substr(0,0)}", longJohn);
    Add("start-neg9-whole-string", "{0:substr(-9)}", longJohn);
    Add("start-at-end-length-neg9", "{0:substr(9,-9)}", longJohn);
    Add("aligned-empty-result", "[{0,6:substr(999)}]", longJohn);
    Add("format-two-placeholders", "{0:substr(0,4):{}-{}}", longJohn);
    Add("from-map", "{Text:substr(0,3)}", """{"Text":"Long John"}""");
    Add("astral-inside-pair", "{0:substr(1,1)}", astral);
    Add("astral-spanning-pair", "{0:substr(1,3)}", astral);
    Add("empty-string", "{0:substr(0)}", @"[""""]");
    Add("empty-string-length", "{0:substr(0,1)}", @"[""""]");

    // -- The error text, which only OutputErrorInResult makes observable.
    var output = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);
    cases.Add(new GoldenCase("substr-errtext-non-string", "{0:substr(0,2)}", number, output));
    cases.Add(new GoldenCase("substr-errtext-bad-option", "{0:substr(x,y)}", longJohn, output));
    cases.Add(new GoldenCase("substr-errtext-overflow", "{0:substr(2147483648)}", longJohn, output));
    cases.Add(new GoldenCase("substr-errtext-out-of-range", "{0:substr(-999)}", longJohn, output));
    cases.Add(new GoldenCase("substr-errtext-format-is-text", "{0:substr(0,2):x}", longJohn, output));

    // -- Options the parser never reaches, and whitespace it does not skip.
    // `int.Parse` runs on the first two options only, so a third is ignored
    // however it is spelled.
    Add("third-option-unparsed", "{0:substr(1,2,x)}", longJohn);
    // An empty second option is not a number: .NET's `Number.IsWhite` skips
    // ASCII whitespace only, so a non-breaking space is not whitespace either.
    cases.Add(new GoldenCase("substr-errtext-empty-length", "{0:substr(-4,)}", longJohn, output));
    cases.Add(new GoldenCase("substr-errtext-nbsp-start", "{0:substr(\u00A01)}", longJohn, output));

    // -- NullDisplayString, which only a null value reaches.
    var nullDisplay = new CaseSettings(SubStringNullDisplayString: "???");
    Add("nulldisplay", "{0:substr(0,3)}", nul, nullDisplay);
    Add("nulldisplay-aligned", "[{0,10:substr(0,3)}]", nul, nullDisplay);
    // A child format is written instead of the null string, against a null
    // value — so the string never appears.
    Add("nulldisplay-child-format-wins", "{0:substr(0,3):[{}]}", nul, nullDisplay);
    Add("nulldisplay-not-null", "{0:substr(0,3)}", longJohn, nullDisplay);

    // -- SplitChar, which separates the options and nothing else.
    var pipeSplit = new CaseSettings(SubStringSplitChar: '|');
    Add("splitchar-pipe", "{0:substr(-4|-1)}", longJohn, pipeSplit);
    cases.Add(new GoldenCase("substr-errtext-splitchar-moved", "{0:substr(-4,-1)}", longJohn,
        new CaseSettings(SubStringSplitChar: '|',
            FormatErrorAction: FormatErrorAction.OutputErrorInResult)));

    // -- CanAutoDetect, off by default. With it on, an unnamed placeholder
    // whose options parse as a substring is claimed by this formatter.
    var autoDetect = new CaseSettings(SubStringCanAutoDetect: true);
    Add("autodetect-on", "{0:(0,4)}", longJohn, autoDetect);
    // It still declines what it cannot slice: a non-string, and options that
    // are empty. Declining while unnamed is not an error, so the next
    // formatter — here the default one — gets its turn.
    Add("autodetect-on-declines-non-string", "{0:(0,4)}", "[true]", autoDetect);
    Add("autodetect-on-declines-empty-options", "{0:()}", longJohn, autoDetect);
    // Off, the same placeholder never reaches this formatter.
    Add("autodetect-off", "{0:(0,4)}", longJohn);

    // -- A divergence: two halves of one surrogate pair, written next to each
    // other. .NET keeps each half as a UTF-16 code unit, so the pair re-forms.
    Add("astral-halves-rejoin", "{0:substr(0,1)}{0:substr(1,1)}", JsonString("\U0001F600"));
    Add("astral-halves-rejoin-child-format",
        "{0:substr(0,1):{}}{0:substr(1,1):{}}", JsonString("\U0001F600"));
    // The controls: halves in the wrong order, and halves with text between
    // them, do not join in .NET either.
    Add("astral-halves-reversed", "{0:substr(1,1)}{0:substr(0,1)}", JsonString("\U0001F600"));
    Add("astral-halves-separated", "{0:substr(0,1)}x{0:substr(1,1)}", JsonString("\U0001F600"));
}

// ---------------------------------------------------------------------------
// M3: NullFormatter, named "isnull".
// ---------------------------------------------------------------------------

static void IsNullCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("isnull-" + id, template, argsJson, settings));

    var values = new (string Slug, string Json)[]
    {
        ("null", "[null]"),
        ("string", @"[""a string""]"),
        ("empty-string", @"[""""]"),
        ("int", "[123]"),
        ("bool", "[true]"),
    };

    var templates = new (string Slug, string Template)[]
    {
        ("one-format", "{0:isnull:It's null}"),
        ("two-formats", "{0:isnull:It's null|It's not}"),
        ("empty-format", "{0:isnull:}"),
        ("two-empty-formats", "{0:isnull:|}"),
        ("empty-options", "{0:isnull()}"),
        ("blank-options", "{0:isnull( ):x}"),
        ("value-in-null-branch", "{0:isnull:[{}]}"),
        ("value-in-value-branch", "{0:isnull:x|[{}]}"),
        ("aligned-one-format", "{0,10:isnull:N}"),
        ("aligned-two-formats", "{0,10:isnull:N|Y}"),
        ("aligned-left", "{0,-3:isnull:N}"),
    };

    foreach (var (templateSlug, template) in templates)
    foreach (var (valueSlug, json) in values)
        Add($"{templateSlug}-{valueSlug}", template, json);

    // Nesting: the value branch runs another formatter over the same value.
    const string nullable = """{"NullableInt":null,"IntValueIfNotNull":1000}""";
    const string nonNull = """{"NullableInt":2000,"IntValueIfNotNull":2000}""";
    const string other = """{"NullableInt":1234.5,"IntValueIfNotNull":1234.5}""";
    const string nested =
        "{NullableInt:isnull:Was null|{IntValueIfNotNull:choose(1000|2000):1k|{:N2}}}";
    Add("nested-null", nested, nullable);
    Add("nested-2000", nested, nonNull);
    Add("nested-else", nested, other);
    Add("nested-substr", "{0:isnull:nothing|{:substr(0,4)}}", @"[""Long John""]");
    Add("nested-list", "{0:isnull:nothing|{:list:{}|, }}", """[["a","b"]]""");

    // Values whose "null" is a selector's doing rather than the argument's.
    Add("map-value", "{0:isnull:N|Y}", """[{"a":1}]""");
    Add("list-value", "{0:isnull:N|Y}", "[[1,2]]");
    Add("null-member", "{City:isnull:no city|{}}", """{"City":null,"Name":"Alice"}""");
    Add("nullable-member", "{City?.Length:isnull:N|Y}", """{"City":null}""");
    Add("in-list", "{0:list:{:isnull:-|{}}|,}", @"[[""a"",null,""b""]]");

    // Errors.
    Add("err-choose-options", "{0:isnull(op|ti|ons):Is null}", "[null]");
    Add("err-three-formats", "{0:isnull:1|2|3}", "[null]");
    Add("err-escaped-split-char", @"{0:isnull:a\|b|c}", "[null]");
    Add("err-short-name", "{0:null:a|b}", "[null]");

    var output = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);
    cases.Add(new GoldenCase(
        "isnull-errtext-choose-options", "{0:isnull(op|ti|ons):Is null}", "[null]", output));
    cases.Add(new GoldenCase(
        "isnull-errtext-three-formats", "{0:isnull:1|2|3}", "[null]", output));

    // The lazy split again: only the piece the formatter picks is cut, so a
    // crossed literal in the branch not taken never fails.
    Add("crossed-first-branch-null", @"{0:isnull:\u12|a}", "[null]");
    Add("crossed-first-branch-value", @"{0:isnull:\u12|a}", @"[""v""]");
    Add("crossed-second-branch-null", @"{0:isnull:a|\u12}", "[null]");
    Add("crossed-second-branch-value", @"{0:isnull:a|\u12}", @"[""v""]");
    Add("crossed-third-branch", @"{0:isnull:a|b|\u12}", "[null]");

    // -- The two knobs on the formatter itself.
    var tildeSplit = new CaseSettings(IsNullSplitChar: '~');
    Add("splitchar-tilde-null", "{0:isnull:N~Y}", "[null]", tildeSplit);
    Add("splitchar-tilde-value", "{0:isnull:N~Y}", @"[""v""]", tildeSplit);
    // The pipe is an ordinary character once the split char has moved, so
    // this is one format and the value branch has nothing to write.
    Add("splitchar-tilde-pipe-is-literal", "{0:isnull:N|Y}", "[null]", tildeSplit);

    // -- CanAutoDetect, off by default. On, an unnamed two-part format is
    // claimed by this formatter — but only after list, plural and cond, which
    // all rank ahead of it and auto-detect the same shape.
    var autoDetect = new CaseSettings(IsNullCanAutoDetect: true);
    Add("autodetect-on-null", "{0:N|Y}", "[null]", autoDetect);
    Add("autodetect-on-string", "{0:N|Y}", @"[""v""]", autoDetect);
    Add("autodetect-on-int", "{0:N|Y}", "[123]", autoDetect);
    Add("autodetect-on-list", "{0:N|Y}", """[["a","b"]]""", autoDetect);
    Add("autodetect-off-null", "{0:N|Y}", "[null]");
}

// ---------------------------------------------------------------------------
// M3: IsMatchFormatter, named "ismatch".
//
// Patterns here stay inside the subset fancy-regex and .NET both read the same
// way; the ones they disagree on are pinned by unit tests in `ismatch.rs`
// instead, with one held here as a knowingly-skipped case.
// ---------------------------------------------------------------------------

static void IsMatchCases(List<GoldenCase> cases)
{
    void Add(string id, string template, string argsJson, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("ismatch-" + id, template, argsJson, settings));

    const string theValue = """{"theValue":"Some123Content"}""";

    // -- The two branches, and what each may write.
    Add("match-with-value", @"{theValue:ismatch(^.+123.+$):Okay - {}|No match content}", theValue);
    Add("match-fixed-text", @"{theValue:ismatch(^.+123.+$):Fixed content if match|No match content}",
        theValue);
    Add("no-match", @"{theValue:ismatch(^.+999.+$):{}|No match content}", theValue);
    Add("match-empty-branch", @"{theValue:ismatch(^.+123.+$):|Only content with no match}", theValue);
    Add("no-match-branch", @"{theValue:ismatch(^.+999.+$):|Only content with no match}", theValue);

    // -- Capture groups, read through the `m` placeholder.
    Add("groups-all", @"{theValue:ismatch(^.+\(1\)\(2\)\(3\).+$):Matches for '{}'\: {m[0]} - {m[1]} - {m[2]} - {m[3]}|No match}",
        theValue);
    Add("groups-two", @"{theValue:ismatch(^.+\(1\)\(2\)\(3\).+$):First 2 matches in '{}'\: {m[1]} and {m[2]}|No match}",
        theValue);
    Add("groups-no-match", @"{theValue:ismatch(^.+\(9\)\(9\)\(9\).+$):Matches for '{}'\: {m[1]}|No match}",
        theValue);
    Add("group-zero", @"{theValue:ismatch(123):[{m[0]}]|no}", theValue);
    Add("group-not-participating", @"{theValue:ismatch(\(123\)|\(zzz\)):[{m[1]}][{m[2]}]|no}", theValue);
    Add("outer-selector-in-branch", @"{theValue:ismatch(123):got {} and {theValue}|no}", theValue);
    Add("group-out-of-range", @"{theValue:ismatch(123):[{m[5]}]|no}", theValue);

    // -- Escapes in the options, where `(`, `)`, `{`, `}`, `:` and `\` all
    // have to be written escaped for the parser to hand them to the engine.
    foreach (var (slug, options, search) in new (string, string, string)[]
             {
                 ("pipe", @"\\|", "|"),
                 ("question", @"\\?", "?"),
                 ("plus", @"\\+", "+"),
                 ("star", @"\\*", "*"),
                 ("caret", @"\\^", "^"),
                 ("dollar", @"\\$", "$"),
                 ("dot", @"\\.", "."),
                 ("open-bracket", @"\\[", "["),
                 ("close-bracket", @"\\]", "]"),
                 ("colon", @"\:", ":"),
                 ("backslash", @"\\\\", @"\"),
                 ("open-paren", @"\\\(", "("),
                 ("close-paren", @"\\\)", ")"),
                 ("open-brace", @"\\\{", "{"),
                 ("close-brace", @"\\\}", "}"),
             })
        Add("escape-" + slug, "{0:ismatch(" + options + "):found {}|}", JsonString(search));

    // -- Pattern syntax both engines read the same way.
    foreach (var (slug, options, search) in new (string, string, string)[]
             {
                 ("parens-group", @"\\\(\([^\\\)]*\)\\\)", "Text (inside) parenthesis"),
                 ("parens-group-no-match", @"\\\(\([^\\\)]*\)\\\)", "No parenthesis"),
                 ("lookahead", @"Lon\(?=don\)", "This is London"),
                 ("lookahead-no-match", @"Lon\(?=don\)", "This is Loando"),
                 ("angle-brackets", "<[^<>]+>", "<abcde>"),
                 ("angle-brackets-no-match", "<[^<>]+>", "<>"),
                 ("open-repetition", @"\\d\{3,\}", "1234"),
                 ("open-repetition-no-match", @"\\d\{3,\}", "12"),
                 ("counted-and-escaped-colon", @"^.\{5,\}\:,$", "1z%aW:,"),
                 ("counted-and-escaped-colon-no-match", @"^.\{5,\}\:,$", "1z:,"),
                 ("backreference", @"\(a\)\\1", "aa"),
                 ("named-backreference", @"\(?<x>a\)\\k<x>", "aa"),
                 ("atomic-group", @"\(?>a+\)b", "ab"),
                 ("inline-comment", @"a\(?#hi\)b", "ab"),
                 ("conditional", @"^\(a\)?\(?\(1\)b|c\)$", "ab"),
                 ("end-of-string-lower-z", @"^ab\\z", "ab"),
                 ("start-of-string-upper-a", @"\\Aab", "abc"),
                 ("word-boundary", @"\\bcat\\b", "a cat sat"),
                 ("word-boundary-no-match", @"\\bcat\\b", "concatenate"),
                 ("digit-class", @"^\\d+$", "1234"),
                 ("word-class", @"^\\w+$", "a_1"),
                 ("whitespace-class", @"^\\s+$", " \t"),
                 ("exact-quantifier", @"^a\{2\}$", "aa"),
                 ("exact-quantifier-no-match", @"^a\{2\}$", "aaa"),
                 ("lookbehind", @"\(?<=a\)b", "ab"),
                 ("negative-lookahead", @"a\(?!b\)", "ac"),
                 ("negative-lookahead-no-match", @"a\(?!b\)", "ab"),
                 ("nested-groups", @"\(\(a\)\(b\)\)", "ab"),
                 ("unicode-letter", @"^\\p\{L\}+$", "abcé"),
                 ("non-greedy", @"^<.+?>", "<a><b>"),
             })
        Add("pattern-" + slug, "{0:ismatch(" + options + "):found {}|}", JsonString(search));

    // Group numbering, and a branch that runs another formatter.
    Add("alternation-groups", @"{0:ismatch(^\(a|b\)\(c|d\)$):[{m[1]}][{m[2]}]|no}", JsonString("ad"));
    Add("nested-group-numbers", @"{0:ismatch(\(\(a\)\(b\)\)):[{m[1]}][{m[2]}][{m[3]}]|no}",
        JsonString("ab"));
    Add("empty-value", "{0:ismatch(^$):empty|not}", @"[""""]");
    Add("nested-ismatch", "{0:ismatch(a):{:ismatch(b):ab|a only}|none}", JsonString("ab"));
    Add("in-list", "{0:list:{:ismatch(a):A|B}|,}", @"[[""a"",""b"",""a""]]");
    Add("aligned-group", @"[{theValue,12:ismatch(123):{m[0]}|no}]", theValue);
    // The match list is a collection, so the other extensions reach it.
    Add("matches-as-list", @"{theValue:ismatch(^.+\(1\)\(2\)\(3\).+$):[{m:list:{}|-}]|no}", theValue);
    Add("matches-with-index", @"{theValue:ismatch(^.+\(1\)\(2\)\(3\).+$):{m:list:{Index}={}|,}|no}",
        theValue);
    Add("group-through-substr", @"{theValue:ismatch(\(Some\)):{m[1]:substr(0,2)}|no}", theValue);

    // The lenient error actions over a format that is not two parts.
    cases.Add(new GoldenCase(
        "ismatch-ignore-one-part", "[{theValue:ismatch(123):one}]", theValue,
        new CaseSettings(FormatErrorAction: FormatErrorAction.Ignore)));
    cases.Add(new GoldenCase(
        "ismatch-maintaintokens-one-part", "[{theValue:ismatch(123):one}]", theValue,
        new CaseSettings(FormatErrorAction: FormatErrorAction.MaintainTokens)));

    // The Unicode currency-symbol category, which both engines know.
    foreach (var (slug, value) in new (string, string)[]
             {
                 ("euro", "\u20ac Euro"), ("yen", "\u00a5 Yen"), ("none", "none"),
             })
        Add("unicode-category-" + slug,
            @"{Currency:ismatch(\\p\{Sc\}):Currency: {m[0]}|Unknown}",
            $$"""{"Currency":{{JsonSerializer.Serialize(value)}}}""");

    // -- Values that are not strings, and an empty pattern.
    Add("empty-pattern", "{0:ismatch():yes [{m[0]}]|no}", JsonString("abc"));
    Add("alignment", "{0,10:ismatch(b):Y|N}", JsonString("abc"));
    Add("bool-value", "{0:ismatch(True):yes|no}", "[true]");
    // No floating-point value belongs here: `IsMatchFormatter` matches against
    // the value's `ToString()` under the *thread* culture, so `1.5` renders
    // with whatever decimal separator the machine has. Same rule as the
    // `choose` options (see `tools/goldens/README.md`).
    Add("int-value-group", @"{0:ismatch(^\(1\)2$):a {m[1]}|b}", "[12]");
    Add("null-value", "{0:ismatch(^.+$):a|b}", "[null]");

    // -- RegexOptions, which is a property of the formatter rather than of the
    // template, so each of these runs with its own configured extension.
    void Options(string id, string template, string argsJson, RegexOptions options) =>
        cases.Add(new GoldenCase(
            "ismatch-" + id, template, argsJson, new CaseSettings(RegexOptions: options)));

    Add("case-sensitive-default", "{theValue:ismatch(^SOME123.+$):Okay - {}|No match content}", theValue);
    Options("ignore-case", "{theValue:ismatch(^SOME123.+$):Okay - {}|No match content}", theValue,
        RegexOptions.IgnoreCase);
    Options("ignore-case-alternation", @"{0:ismatch(^A|B$):yes|no}", JsonString("b"),
        RegexOptions.IgnoreCase);
    Add("multiline-default", "{0:ismatch(^b$):yes|no}", JsonString("a\nb\nc"));
    Options("multiline", "{0:ismatch(^b$):yes|no}", JsonString("a\nb\nc"), RegexOptions.Multiline);
    Options("singleline", "{0:ismatch(^a.b$):yes|no}", JsonString("a\nb"), RegexOptions.Singleline);
    Options("ignore-pattern-whitespace", "{0:ismatch(^a b c$):yes|no}", JsonString("abc"),
        RegexOptions.IgnorePatternWhitespace);
    Options("ignore-pattern-whitespace-in-class", "{0:ismatch(^[a b]+$):yes|no}", JsonString("a b"),
        RegexOptions.IgnorePatternWhitespace);
    Options("compiled-is-a-hint", "{theValue:ismatch(^.+123.+$):Okay|No}", theValue,
        RegexOptions.Compiled);
    Options("culture-invariant", "{theValue:ismatch(^SOME123.+$):Okay|No}", theValue,
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    // A different split character, and a different name for the match list.
    var tilde = new CaseSettings(IsMatchSplitChar: '~');
    cases.Add(new GoldenCase(
        "ismatch-split-char-match", @"{theValue:ismatch(^.+123.+$):|Has match for '{}'|~|No match|}",
        theValue, tilde));
    cases.Add(new GoldenCase(
        "ismatch-split-char-no-match", @"{theValue:ismatch(^.+999.+$):|Has match for '{}'|~|No match|}",
        theValue, tilde));
    cases.Add(new GoldenCase(
        "ismatch-placeholder-name",
        @"{theValue:ismatch(^\(123\)\(4\)\(5\)$):First match in '{}'\: {match[1]}|No match}",
        """{"theValue":"12345"}""",
        new CaseSettings(IsMatchPlaceholderName: "match")));

    // -- Errors: a format that is not exactly two parts, and an option escape
    // that cannot be resolved.
    Add("err-one-part", "{theValue:ismatch(^.+123.+$):Dummy content}", theValue);
    Add("err-three-parts", "{theValue:ismatch(^.+123.+$):a|b|c}", theValue);
    Add("err-empty-format", "{theValue:ismatch(^.+123.+$):}", theValue);
    Add("err-no-format", "{theValue:ismatch(^.+123.+$)}", theValue);
    Add("err-bad-escape", @"{0:ismatch(a\qb):yes|no}", JsonString("x"));

    var output = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);
    cases.Add(new GoldenCase(
        "ismatch-errtext-one-part", "{theValue:ismatch(^.+123.+$):Dummy content}", theValue, output));
    cases.Add(new GoldenCase(
        "ismatch-errtext-no-format", "{theValue:ismatch(^.+123.+$)}", theValue, output));
    cases.Add(new GoldenCase(
        "ismatch-errtext-null-value", "{0:ismatch(^.+$):a|b}", "[null]", output));

    // -- Every item of a matched format takes the placeholder's alignment,
    // the literals included.
    Add("aligned-multi-item", "{0,10:ismatch(b):x{}y|N}", JsonString("abc"));

    // -- CanAutoDetect, off by default. On, an unnamed placeholder is offered
    // to this formatter, which declines rather than fails when it cannot use
    // the format.
    var autoDetect = new CaseSettings(IsMatchCanAutoDetect: true);
    Add("autodetect-on", "{0:yes|no}", JsonString("abc"), autoDetect);
    Add("autodetect-on-one-part", "{0:only}", JsonString("abc"), autoDetect);
    Add("autodetect-off", "{0:yes|no}", JsonString("abc"));

    // -- Documented divergences, held here with .NET's answer and skipped by
    // the runner. .NET's `$` also matches before a final newline, where
    // fancy-regex's does not.
    Add("dollar-before-final-newline", "{0:ismatch(^abc$):yes|no}", JsonString("abc\n"));
    // .NET matches over UTF-16 code units, so an astral character is two of
    // them; fancy-regex matches over scalars, where it is one.
    Add("astral-dot", "{0:ismatch(^.$):yes|no}", JsonString("\U0001F600"));
    Add("astral-two-dots", "{0:ismatch(^..$):yes|no}", JsonString("\U0001F600"));
    Add("astral-negated-class", "{0:ismatch(^[^a]$):yes|no}", JsonString("\U0001F600"));
    Add("astral-captured-group", @"{0:ismatch(^\(.\).*$):[{m[1]}]|no}",
        JsonString("\U0001F600abc"));
    // .NET's `\w` does not include letter numbers, spacing marks or enclosing
    // marks; the `regex` crate's does.
    Add("word-letter-number", @"{0:ismatch(^\\w+$):yes|no}", JsonString("Ⅷ"));
    Add("word-spacing-mark", @"{0:ismatch(^\\w+$):yes|no}", JsonString("ः"));
    Add("word-boundary-letter-number", @"{0:ismatch(\\bx\\b):yes|no}",
        JsonString("ⅧxⅧ"));
    // `\d` and `\s` agree, so these are ordinary cases rather than skips.
    Add("digit-arabic-indic", @"{0:ismatch(^\\d+$):yes|no}", JsonString("٣"));
    Add("whitespace-nel", @"{0:ismatch(^\\s$):yes|no}", JsonString("\u0085"));
    // Simple case folding is wider than .NET's simple case mapping, and
    // CultureInvariant does not reconcile them.
    Options("fold-long-s", "{0:ismatch(^s$):yes|no}", JsonString("ſ"),
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    Options("fold-final-sigma", "{0:ismatch(^σ$):yes|no}", JsonString("ς"),
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    Options("fold-deseret", "{0:ismatch(^\U00010400$):yes|no}", JsonString("\U00010428"),
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    // The controls that do agree.
    Options("fold-kelvin", "{0:ismatch(^k$):yes|no}", JsonString("K"),
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    Options("fold-capital-sharp-s", "{0:ismatch(^ẞ$):yes|no}", JsonString("ß"),
        RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);
    // `\0` is NUL in .NET; fancy-regex reads it as a back reference.
    Add("nul-escape", @"{0:ismatch(^\\0$):yes|no}", JsonString("\0"));
    // `\x00` is the spelling both engines agree on.
    Add("hex-nul-escape", @"{0:ismatch(^\\x00$):yes|no}", JsonString("\0"));
    // Class intersection, POSIX names and nesting, which .NET reads as
    // literal characters.
    Add("class-intersection", @"{0:ismatch(^[a&&b]+$):yes|no}", JsonString("ab"));
    Add("class-posix-name", @"{0:ismatch(^[[\:alpha\:]]+$):yes|no}", JsonString("abc"));
    Add("class-nested", @"{0:ismatch(^[a[bc]]+$):yes|no}", JsonString("abc"));
    // An octal escape is the loud kind: fancy-regex refuses to compile it.
    Add("octal-escape", @"{0:ismatch(^\\101$):yes|no}", JsonString("A"));
}

// ---------------------------------------------------------------------------
// M3: TemplateFormatter, named "t".
//
// Not in `CreateDefaultSmartFormat`, so each case names the template fixture it
// runs with (see `TemplateFixture`).
// ---------------------------------------------------------------------------

static void TemplateCases(List<GoldenCase> cases)
{
    var standard = new CaseSettings(Templates: TemplateSet.Standard);
    var withEmpty = new CaseSettings(Templates: TemplateSet.WithEmptyName);
    var insensitive = new CaseSettings(
        Templates: TemplateSet.CaseInsensitive,
        CaseSensitivity: CaseSensitivityType.CaseInsensitive);
    var output = new CaseSettings(
        Templates: TemplateSet.Standard,
        FormatErrorAction: FormatErrorAction.OutputErrorInResult);

    const string person = """{"First":"Scott","Last":"Rippey"}""";

    void Add(string id, string template, string argsJson = person, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("template-" + id, template, argsJson, settings ?? standard));

    // -- Where the name may be written, and what else the placeholder may hold.
    Add("name-in-format", "{:t:firstLast}");
    Add("name-in-options", "{:t(firstLast)}");
    Add("empty-options-name-in-format", "{:t():firstLast}");
    Add("options-win-over-format", "{:t(firstLast):IGNORED}");
    Add("long-name-is-unknown", "{:template:firstLast}");
    Add("selector-before-name", "{First:t:lastFirst}");

    // -- Lookups.
    Add("last-first", "{:t:lastFirst}");
    Add("upper-first", "{:t:FIRST}");
    Add("lower-last", "{:t:last}");
    Add("upper-last", "{:t:LAST}");
    Add("nested", "{:t:NESTED}");
    cases.Add(new GoldenCase("template-case-insensitive-name", "{:T:firstLast}", person, insensitive));
    cases.Add(new GoldenCase("template-case-insensitive-last", "{:t:LAST}", person, insensitive));
    cases.Add(new GoldenCase("template-case-insensitive-nested", "{:t:NeStEd}", person, insensitive));
    cases.Add(new GoldenCase("template-case-insensitive-nested-lower", "{:t:nested}", person, insensitive));
    Add("case-sensitive-wrong-case", "{:t:LaSt}");

    // -- Unknown names.
    Add("unknown-name", "{:t:does-not-exist}");
    Add("unknown-name-in-options", "{:t(nope)}");
    Add("empty-name-unregistered", "{:t:}");
    Add("empty-options-unregistered", "{:t()}");
    Add("empty-options-and-format-unregistered", "{:t():}");
    cases.Add(new GoldenCase("template-empty-name", "{:t:}", person, withEmpty));
    cases.Add(new GoldenCase("template-empty-options", "{:t()}", person, withEmpty));
    cases.Add(new GoldenCase("template-empty-options-and-format", "{:t():}", person, withEmpty));

    // The error text, which quotes the name as it resolved.
    cases.Add(new GoldenCase("template-errtext-unknown", "{:t:does-not-exist}", person, output));
    cases.Add(new GoldenCase("template-errtext-empty-name", "{:t:}", person, output));
    cases.Add(new GoldenCase(
        "template-errtext-unknown-case-insensitive", "{:T:nope}", person,
        new CaseSettings(
            Templates: TemplateSet.CaseInsensitive,
            CaseSensitivity: CaseSensitivityType.CaseInsensitive,
            FormatErrorAction: FormatErrorAction.OutputErrorInResult)));

    // -- The name is a literal, so its escape sequences resolve like any other.
    Add("escaped-backslash", @"{:t:back\\slash}");
    Add("escaped-backslash-in-options", @"{:t(back\\slash)}");
    Add("escaped-braces", @"{:t:\{brace\}}");
    Add("pipe-in-name", "{:t:a|b}");
    Add("pipe-in-options", "{:t(a|b)}");
    Add("unresolvable-escape", @"{:t:back\slash}");
    Add("crossed-escape", @"{:t:a\u12}");
    Add("unicode-escape", @"{:t:\u0041}");

    // -- Alignment reaches the template's literal text, not the whole result.
    Add("alignment-right", "[{,20:t(firstLast)}]");
    Add("alignment-left", "[{,-20:t(firstLast)}]");

    // -- A template over a selected value, and two in one template string.
    Add("selected-value", "{Person:t:firstLast}",
        """{"Person":{"First":"Scott","Last":"Rippey"}}""");
    Add("twice", "{:t:firstLast} / {:t:lastFirst}");
    Add("around-literals", "<{:t:FIRST}>");
    Add("name-with-space", "{:t:first Last}");
    Add("name-with-colon", @"{:t:a\:b}");

    // -- Nesting: a template that names two more, and one that runs `cond`
    // over a second positional argument.
    cases.Add(new GoldenCase(
        "template-cond-formal", "{0:t(salutation)}:",
        """[{"First":"Joe","Last":"Doe"},true]""", standard));
    cases.Add(new GoldenCase(
        "template-cond-informal", "{0:t(salutation)}:",
        """[{"First":"Joe","Last":"Doe"},false]""", standard));

    // -- A template rendered for every item of a list.
    cases.Add(new GoldenCase(
        "template-in-list", "{:{:t:NESTED}|, }",
        """[[{"First":"Jim","Last":"Halpert"},{"First":"Pam","Last":"Beasley"},{"First":"Dwight","Last":"Schrute"}]]""",
        standard));

    // -- A template that reads the list index of the list it is rendered in.
    cases.Add(new GoldenCase(
        "template-in-list-with-index", "{:{:t:indexed}|, }",
        """[[{"First":"Jim"},{"First":"Pam"}]]""", standard));

    // -- The lenient error actions over an unknown template name.
    cases.Add(new GoldenCase(
        "template-ignore-unknown", "[{:t:nope}]", person,
        new CaseSettings(Templates: TemplateSet.Standard, FormatErrorAction: FormatErrorAction.Ignore)));
    cases.Add(new GoldenCase(
        "template-maintaintokens-unknown", "[{:t:nope}]", person,
        new CaseSettings(
            Templates: TemplateSet.Standard,
            FormatErrorAction: FormatErrorAction.MaintainTokens)));

    // -- In compatibility mode no formatter name is parsed at all.
    cases.Add(new GoldenCase(
        "template-compatibility-mode", "{0:t:x}", @"[""a string""]",
        new CaseSettings(Templates: TemplateSet.Simple, StringFormatCompatibility: true)));

    // -- Divergences, held here with .NET's answer. .NET reports an error
    // raised inside a template against the *template*'s text; we quote the
    // string being rendered.
    cases.Add(new GoldenCase("template-error-inside", "x{:t:bad}y", person, output));
}

// ---------------------------------------------------------------------------
// Cases that leave `ListFormatter.CollectionIndex` set, which in .NET is a
// static: an iteration that fails part-way through never runs the restore, and
// every later `{Index}` in the *process* — under any settings, through any
// formatter instance — then reads the leaked value instead of -1. Nothing may
// follow these, and the render loop's canary enforces it.
//
// The port has no such leak (its index is a per-call cell), so it agrees with
// .NET on the case itself and not on what comes after; which is why nothing
// comes after.
// ---------------------------------------------------------------------------

static void CollectionIndexPoisoningCases(List<GoldenCase> cases)
{
    // A spacer whose escape cannot be resolved fails when it is written, after
    // the first item has already reached the output.
    cases.Add(new GoldenCase(
        "list-spacer-bad-escape", @">{0:list:{}|a\qb}<", """[["a","b","c"]]""",
        new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult)));
}

// ---------------------------------------------------------------------------
// M4: TimeFormatter, from the SmartFormat.Extensions.Time package.
//
// Every case runs a `TimeSpan` — or, where it says so, a `DateTime` measured
// against the pinned clock — through the `time` formatter. The fixture spans
// are the ones SmartFormat.NET's own TimeFormatterTests use, which is what
// makes their expected words checkable against that test file.
// ---------------------------------------------------------------------------

static void TimeCases(List<GoldenCase> cases)
{
    const string zero = "00:00:00";
    const string oneOfEach = "1.01:01:01.0010000";
    const string negOneOfEach = "-1.01:01:01.0010000";
    const string twoHoursTwoSeconds = "0.02:00:02";
    const string threeDaysThreeSeconds = "3.00:00:03";
    const string fourMilliseconds = "00:00:00.0040000";
    const string fiveDays = "5.00:00:00";
    const string seventyDays = "70.00:00:00";
    const string maxValue = "10675199.02:48:05.4775807";
    const string minValue = "-10675199.02:48:05.4775808";

    // The six languages the extension ships resource files for.
    string[] languages = ["en", "de", "es", "fr", "it", "pt"];

    var fixture = new (string Slug, string Span)[]
    {
        ("zero", zero),
        ("one-of-each", oneOfEach),
        ("two-hours", twoHoursTwoSeconds),
        ("three-days", threeDaysThreeSeconds),
        ("four-ms", fourMilliseconds),
        ("five-days", fiveDays),
    };

    var outputError = new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult);

    void Add(string id, string template, string span, string culture = "",
        CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("time-" + id, template, TimeSpanArgs(span), settings, culture));

    // The default options — days down to seconds, every zero unit dropped —
    // over the fixture, in the two languages whose wording differs most.
    foreach (var culture in new[] { "en", "de" })
    foreach (var (slug, span) in fixture)
        Add($"default-{culture}-{slug}", "{0:time:}", span, culture);

    foreach (var language in languages)
    {
        // Every unit word and every plural arm of the language at once: `full`
        // keeps the zero units and `weeks milliseconds` opens the whole range.
        // The negative span is safe to pin because the number is written with
        // the invariant culture, whose negative sign is a hyphen.
        Add($"full-{language}-pos", $"{{0:time({language}):weeks milliseconds full}}", oneOfEach);
        Add($"full-{language}-neg", $"{{0:time({language}):weeks milliseconds full}}", negOneOfEach);
        // The "less than" texts, spelled out and abbreviated.
        Add($"zero-{language}-weeks", $"{{0:time({language}):weeks}}", zero);
        Add($"zero-{language}-abbr-ms", $"{{0:time({language}):abbr milliseconds}}", zero);
    }

    // The truncation matrix over a span with a zero unit in the middle.
    foreach (var truncate in new[] { "auto", "short", "fill", "full" })
    {
        Add($"truncate-{truncate}", $"{{0:time:days milliseconds {truncate}}}", twoHoursTwoSeconds);
        Add($"truncate-{truncate}-abbr", $"{{0:time:days milliseconds abbr {truncate}}}",
            twoHoursTwoSeconds);
    }

    // Rounding: the default drops what is below the smallest unit, `noless`
    // rounds it up.
    Add("round-floor", "{0:time:}", oneOfEach);
    Add("round-ceiling", "{0:time:noless}", oneOfEach);
    Add("round-neg-ceiling", "{0:time:weeks milliseconds full noless}", negOneOfEach);

    // `TotalMilliseconds` saturates an `int` cast, and the widest units still
    // fit where the default range overflows.
    Add("saturate-milliseconds", "{0:time:milliseconds}", seventyDays);
    Add("min-milliseconds", "{0:time:milliseconds}", minValue);
    Add("max-weeks", "{0:time:weeks}", maxValue);
    Add("min-overflow", "{0:time:}", minValue);
    Add("max-overflow", "{0:time:noless weeks}", maxValue);
    Add("min-overflow-text", "{0:time:}", minValue, settings: outputError);
    Add("max-overflow-text", "{0:time:noless weeks}", maxValue, settings: outputError);

    // A nested format hands the unit texts to the `list` formatter. .NET drops
    // the format's first item whatever it is, so the leading space is load
    // bearing — without it the list placeholder itself is what goes.
    foreach (var (slug, span) in fixture)
        Add($"list-{slug}", "{0:time: {:list:|, | and }}", span);
    Add("list-no-leading-literal", "{0:time:{:list:|, | and }}", oneOfEach);
    Add("list-trailing-word", "{0:time: {:list:|, | and } hours}", oneOfEach);

    // SmartFormat 2.x put the format in the options and left the format empty.
    // The two spellings agree, and the compatibility branch means a culture can
    // never be named that way.
    Add("v2-options-are-the-format", "{0:time(abbr hours noless)}", oneOfEach);
    Add("v3-format", "{0:time:abbr hours noless}", oneOfEach);
    Add("v2-culture-is-swallowed", "{0:time(en):}", oneOfEach, "de");

    // Which culture the unit texts come from.
    Add("culture-from-call", "{0:time:hours minutes}", oneOfEach, "de");
    Add("culture-from-invariant-call", "{0:time:hours minutes}", oneOfEach);
    Add("culture-option-wins", "{0:time(es):hours minutes}", oneOfEach, "de");
    // A culture with no TimeTextInfo falls back to the fallback language.
    Add("culture-unshipped", "{0:time(nl):hours minutes}", oneOfEach, "de");
    // The name is resolved through CultureInfo, so case, a specific culture
    // and an alternate sort order all land on the same language.
    Add("culture-uppercase", "{0:time(EN):hours minutes}", oneOfEach, "de");
    Add("culture-specific", "{0:time(en-US):hours minutes}", oneOfEach, "de");
    Add("culture-alt-sort", "{0:time(en_US):hours minutes}", oneOfEach, "de");
    // A three-letter ISO 639-2/T code, which ICU folds to its two-letter
    // equivalent and `fmt::culture::language_subtag` takes as written: German
    // in .NET, the English fallback here. The documented `language_subtag`
    // gap, seen from this formatter.
    Add("culture-iso-639-2", "{0:time(deu):weeks}", zero);
    Add("culture-iso-639-2-english", "{0:time(eng):weeks}", zero);

    // A value the formatter cannot process. The exception is built from the
    // format's *first item*, so the message quotes the template when the
    // format has one and an empty line when the format is empty.
    foreach (var (slug, json) in new (string Slug, string Json)[]
             {
                 ("string", """["abc"]"""), ("int", "[42]"), ("bool", "[true]"), ("null", "[null]"),
             })
    foreach (var (formatSlug, template) in new (string, string)[]
             { ("format", "ab {0:time:hours}"), ("empty-format", "ab {0:time:}") })
    {
        cases.Add(new GoldenCase($"time-wrong-type-{slug}-{formatSlug}", template, json));
        cases.Add(new GoldenCase(
            $"time-wrong-type-{slug}-{formatSlug}-text", template, json, outputError));
    }

    // A malformed culture name is a plain exception, so its bare message is
    // what reaches the output.
    Add("culture-malformed", "{0:time(xx-YY-):hours}", oneOfEach);
    Add("culture-malformed-text", "{0:time(xx-YY-):hours}", oneOfEach, settings: outputError);

    // A DateTime is the span between the pinned clock and the value, so a past
    // moment is a positive span. The TimeSpan of the same length is next to it.
    foreach (var (slug, hours) in new (string, int)[]
             { ("now", 0), ("past-12h", -12), ("future-12h", 12), ("past-23h", -23), ("future-23h", 23) })
    {
        cases.Add(new GoldenCase(
            $"time-relative-{slug}", "{0:time:abbr hours noless}",
            JsonDateTime(PinnedNow().AddHours(hours))));
        var span = TimeSpan.FromHours(-hours);
        cases.Add(new GoldenCase(
            $"time-relative-{slug}-as-span", "{0:time:abbr hours noless}",
            TimeSpanArgs(span.ToString("c", CultureInfo.InvariantCulture))));
    }
}

// ---------------------------------------------------------------------------
// M4: a TimeSpan through DefaultFormatter, which is .NET's own
// `TimeSpanFormat`: the `c` / `t` / `T` round-trip pattern, and the `g` / `G`
// pair, which take the decimal separator — and nothing else — from the
// culture.
// ---------------------------------------------------------------------------

static void TimeSpanDefaultCases(List<GoldenCase> cases)
{
    var values = new (string Slug, string Span)[]
    {
        ("zero", "00:00:00"),
        ("day", "1.01:01:01.0010000"),
        ("neg-day", "-1.01:01:01.0010000"),
        ("hours", "02:03:04"),
        ("tick", "00:00:00.0000001"),
        ("max", "10675199.02:48:05.4775807"),
        ("min", "-10675199.02:48:05.4775808"),
        ("half-second", "00:00:00.5000000"),
        ("ten-thousand-days", "10000.00:00:00"),
    };

    foreach (var (valueSlug, span) in values)
    foreach (var (specSlug, spec) in new (string, string)[]
             { ("none", ""), ("c", "c"), ("t-lc", "t"), ("T", "T"), ("g-lc", "g"), ("G", "G") })
        cases.Add(new GoldenCase(
            $"tsdefault-{valueSlug}-{specSlug}",
            spec.Length == 0 ? "{0}" : "{0:" + spec + "}",
            TimeSpanArgs(span)));

    // Only the decimal separator moves with the culture: the ':' between the
    // components is a literal even for fi, whose TimeSeparator is '.'.
    foreach (var culture in new[] { "de-DE", "ar-SA", "fi" })
    foreach (var (valueSlug, span) in values.Where(
                 v => v.Slug is "day" or "neg-day" or "tick" or "max"))
    foreach (var (specSlug, spec) in new (string, string)[] { ("c", "c"), ("g-lc", "g"), ("G", "G") })
        cases.Add(new GoldenCase(
            $"tsdefault-{CultureSlug(culture)}-{valueSlug}-{specSlug}",
            "{0:" + spec + "}", TimeSpanArgs(span), Culture: culture));

    // An unknown specifier is .NET's own message.
    cases.Add(new GoldenCase("tsdefault-unknown-spec", "{0:x}", TimeSpanArgs("02:03:04")));
    cases.Add(new GoldenCase(
        "tsdefault-unknown-spec-text", "{0:x}", TimeSpanArgs("02:03:04"),
        new CaseSettings(FormatErrorAction: FormatErrorAction.OutputErrorInResult)));
    // Custom TimeSpan patterns, which .NET renders and the port refuses: the
    // documented custom-pattern non-goal, skipped by the Rust runner.
    cases.Add(new GoldenCase("tsdefault-custom-pattern", @"{0:hh\:mm}", TimeSpanArgs("02:03:04")));
    cases.Add(new GoldenCase("tsdefault-custom-pattern-one-char", "{0:%h}", TimeSpanArgs("02:03:04")));
}

// ---------------------------------------------------------------------------
// M4: LocalizationFormatter, named `L`, over `LocalizationFixture` — a
// dictionary-backed ILocalizationProvider rather than the resx-backed one
// SmartFormat ships, so the table is in the source of both harnesses.
//
// Two shapes are deliberately absent, because the port does not implement them
// yet and a golden for either would be red on arrival: a localized string that
// formats a number or a date *while the options name the culture*, and a key
// that only matches after the format's nested placeholders are rendered.
// ---------------------------------------------------------------------------

static void LocalizationCases(List<GoldenCase> cases)
{
    const string none = "[]";

    void Add(string id, string template, string args = none, string culture = "",
        CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("loc-" + id, template, args, settings, culture));

    // Which culture the key is looked up in: the call's, walking specific →
    // parent → invariant. `pt` has no table at all and reaches the invariant
    // one.
    foreach (var culture in new[] { "", "es", "de", "fr", "pt" })
        Add($"call-culture-{(culture.Length == 0 ? "invariant" : culture)}",
            "{:L:WeTranslateText}", culture: culture);
    Add("invariant-only-key", "{:L:OnlyExistForInvariantCulture}", culture: "pt");

    // The formatter options name a culture, which wins over the call's.
    foreach (var culture in new[] { "es", "en", "fr", "de" })
        Add($"option-culture-{culture}", $"{{:L({culture}):WeTranslateText}}");
    Add("option-culture-beats-call", "{:L(de):WeTranslateText}", culture: "fr-FR");
    // A specific culture falls back to its language's table.
    Add("option-culture-specific", "{:L(es-MX):WeTranslateText}");
    // … through `CultureInfo.Parent`, which for `zh-CN` is the *script*
    // culture `zh-Hans` and not `zh`.
    Add("call-culture-zh-cn", "{:L:WeTranslateText}", culture: "zh-CN");
    Add("option-culture-zh-cn", "{:L(zh-CN):WeTranslateText}");
    Add("option-culture-zh-hans", "{:L(zh-Hans):WeTranslateText}");
    // Empty options are no options, and the options are trimmed.
    Add("option-culture-empty", "{:L():WeTranslateText}");
    Add("option-culture-empty-es", "{:L():WeTranslateText}", culture: "es");
    Add("option-culture-spaces", "{:L( es ):WeTranslateText}");
    Add("option-culture-tab", "{:L(\tes):WeTranslateText}");

    // Errors, under every error action: an empty format, a key nothing
    // translates, a translation that does not parse, and an escape sequence in
    // the key that cannot be resolved.
    foreach (var (slug, template) in new (string, string)[]
             {
                 ("empty-format", "{:L:}"),
                 ("no-format", "{:L()}"),
                 ("empty-options-and-format", "{:L():}"),
                 ("missing-key", "{:L(es):NonExisting}"),
                 ("unparsable-translation", "{:L:BadParse}"),
                 ("bad-escape-in-key", @"{:L:a\qb}"),
                 // A translation that is found and then fails while being
                 // rendered: a selector no source answers, and a nested
                 // `{:L:…}` whose own key has no translation. .NET quotes the
                 // *translation* in the error message and we quote the outer
                 // template, which only the OutputErrorInResult twins see.
                 ("inside-translation", "{:L:greetNobody}"),
                 ("inside-nested-translation", "{:L:OuterMissingInner}"),
             })
    {
        Add($"error-{slug}", template);
        foreach (var action in new[]
                 {
                     FormatErrorAction.Ignore, FormatErrorAction.MaintainTokens,
                     FormatErrorAction.OutputErrorInResult,
                 })
            Add($"error-{slug}-{action.ToString().ToLowerInvariant()}", template,
                settings: new CaseSettings(FormatErrorAction: action));
    }

    // The two knobs of `LocalizationProvider`. The fallback culture is walked
    // as a chain of its own, and only after the requested culture's chain has
    // come up empty, so a key both tables hold still answers from the
    // requested one.
    var fallback = new CaseSettings(Localization: LocalizationSet.Fallback);
    Add("fallback-culture-used", "{:L:OnlyGerman}", culture: "es", settings: fallback);
    Add("fallback-culture-not-needed", "{:L:WeTranslateText}", culture: "es",
        settings: fallback);
    Add("fallback-culture-invariant-call", "{:L:OnlyGerman}", settings: fallback);
    Add("fallback-culture-option", "{:L(fr):OnlyGerman}", settings: fallback);
    Add("fallback-culture-still-missing", "{:L:NonExisting}", culture: "es",
        settings: fallback);
    // Without it, the same key is an error.
    Add("fallback-culture-off", "{:L:OnlyGerman}", culture: "es");

    // `ReturnNameIfNotFound` makes a miss render the key — as a template, so
    // the key's own placeholders resolve.
    var returnName = new CaseSettings(Localization: LocalizationSet.ReturnName);
    Add("return-name-missing-key", "{:L:NonExisting}", settings: returnName);
    Add("return-name-found-key", "{:L(es):WeTranslateText}", settings: returnName);
    Add("return-name-is-a-template", "{:L:Hi {0}}", """["Joe"]""", settings: returnName);

    // The key is the format's RawText, so escape sequences are resolved into
    // it before the lookup.
    Add("key-escaped-brace", @"{:L:a\{b}");

    // Rendering: the alignment of the placeholder reaches the child format …
    Add("alignment", "{,20:L(es):WeTranslateText}");
    // … a translation may localize again …
    Add("nested-localization", "{:L:Outer}");
    // … and a placeholder in a translation resolves against the current scope.
    Add("placeholder-name", "{:L(de):greet}", """{"Name":"Joe"}""");

    // The parse cache is keyed with the settings' comparer, so two
    // translations that differ only in case collide when it ignores case.
    Add("cache-case-sensitive", "{:L:K1}|{:L:K2}", """["x"]""");
    Add("cache-case-insensitive", "{:L:K1}|{:L:K2}", """["x"]""",
        settings: new CaseSettings(CaseSensitivity: CaseSensitivityType.CaseInsensitive));

    // A translation holding a placeholder that formats a number, with the
    // culture coming from the call.
    const string city = """["X-City", 8900000]""";
    foreach (var culture in new[] { "", "de", "fr" })
        Add($"placeholder-number-{(culture.Length == 0 ? "invariant" : culture)}",
            "{0} {1:L:has {:N0} inhabitants}", city, culture);
    foreach (var culture in new[] { "", "es" })
        Add($"placeholder-positional-{(culture.Length == 0 ? "invariant" : culture)}",
            "{:L:{0} has {1:N0} inhabitants}", city, culture);

    // Through the count-driven formatters, whose parts are localized one by
    // one. Both go through a key the raw text already matches.
    foreach (var count in new[] { "0", "1", "200" })
        Add($"cond-items-en-{count}",
            "{0:cond:{:L:{} items}|{:L:{} item}|{:L:{} items}}", "[" + count + "]", "en");
    foreach (var culture in new[] { "es", "fr", "de" })
        Add($"cond-items-{culture}-200",
            "{0:cond:{:L:{} items}|{:L:{} item}|{:L:{} items}}", "[200]", culture);
    foreach (var (culture, count) in new (string, string)[]
             { ("en", "0"), ("en", "1"), ("de", "200"), ("fr", "0"), ("fr", "1"), ("fr", "200") })
        Add($"plural-items-{culture}-{count}",
            "{0:plural:{:L:{} item}|{:L:{} items}}", "[" + count + "]", culture);

    // The formatter's name is matched with the settings' comparer, and `L` is
    // the only name it has — `localize` was a 2.x alias.
    Add("name-localize", "{:localize:WeTranslateText}");
    Add("name-lowercase", "{:l:WeTranslateText}");
    Add("name-lowercase-case-insensitive", "{:l:WeTranslateText}",
        settings: new CaseSettings(CaseSensitivity: CaseSensitivityType.CaseInsensitive));
}

// ---------------------------------------------------------------------------
// M4: the persistent variables source, which answers a group name before any
// other source is asked.
// ---------------------------------------------------------------------------

static void VariablesCases(List<GoldenCase> cases)
{
    const string none = "[]";
    var standard = new CaseSettings(Variables: VariableSet.Standard);
    var standardIgnoringCase = new CaseSettings(
        CaseSensitivity: CaseSensitivityType.CaseInsensitive, Variables: VariableSet.Standard);
    var precedence = new CaseSettings(Variables: VariableSet.Precedence);
    var shadowing = new CaseSettings(Variables: VariableSet.Shadowing);

    void Add(string id, string template, string args = none, CaseSettings? settings = null) =>
        cases.Add(new GoldenCase("var-" + id, template, args, settings ?? standard));

    // Reading a variable out of a group, with no arguments at all.
    Add("group-variable", "{global.theVariable}");
    Add("nested-group", "{global.nested.inner}");
    Add("null-variable", "{global.nullVar}");
    Add("null-variable-nullable-operator", "{global.nullVar?.Any}");

    // What is not there is an error, and the nullable operator on the *group*
    // does not excuse a missing variable. Each one gets the twins under the
    // other error actions, so the message and the caret column are pinned too
    // and not just the exception type.
    foreach (var (slug, template) in new (string, string)[]
             {
                 ("missing-variable", "{global.missing}"),
                 ("missing-group", "{missingGroup}"),
                 ("missing-variable-nullable-group", "{global?.missing}"),
                 ("missing-nested-variable", "{global.nested.missing}"),
             })
    {
        Add(slug, template);
        foreach (var action in new[]
                 {
                     FormatErrorAction.Ignore, FormatErrorAction.MaintainTokens,
                     FormatErrorAction.OutputErrorInResult,
                 })
            Add($"{slug}-{action.ToString().ToLowerInvariant()}", template,
                settings: new CaseSettings(FormatErrorAction: action, Variables: VariableSet.Standard));
    }

    // Group and variable names are matched ordinally, whatever the settings
    // say — .NET's group dictionaries never consult CaseSensitivity.
    Add("group-name-wrong-case", "{GLOBAL.theVariable}");
    Add("variable-name-wrong-case", "{global.THEVARIABLE}");
    foreach (var (slug, template) in new (string, string)[]
             {
                 ("group-name-wrong-case", "{GLOBAL.theVariable}"),
                 ("variable-name-wrong-case", "{global.THEVARIABLE}"),
             })
        Add($"{slug}-outputerrorinresult", template,
            settings: new CaseSettings(
                FormatErrorAction: FormatErrorAction.OutputErrorInResult,
                Variables: VariableSet.Standard));
    Add("group-name-wrong-case-ignoring-case", "{GLOBAL.theVariable}", settings: standardIgnoringCase);
    Add("variable-name-wrong-case-ignoring-case", "{global.THEVARIABLE}",
        settings: standardIgnoringCase);

    // A group written on its own: .NET's DefaultFormatter reaches its
    // ToString(), which is the CLR type name.
    Add("group-as-value", "{global}");
    Add("nested-group-as-value", "{global.nested}");

    // A variable is an ordinary value, so every formatter works on one.
    Add("value-int-n2", "{v.i:N2}");
    Add("value-bool-condition", "{v.b:yes|no}");
    Add("value-string-selector", "{v.s.Length}");
    Add("value-date", "{v.dt:d}");
    Add("value-list", "{v.list:list:{}|, |, and }");
    Add("value-list-index", "{v.list.0}");

    // Precedence against the arguments. A dictionary argument holding the
    // group's name loses in .NET, which looks only for an IVariablesGroup
    // before the group names.
    Add("precedence-no-argument", "{global.theVariable}", settings: precedence);
    Add("precedence-unrelated-argument", "{global.theVariable}", """{"somethingElse":"x"}""",
        precedence);
    Add("precedence-map-argument", "{global.theVariable}",
        """{"global":{"theVariable":"val-from-argument"}}""", precedence);
    Add("precedence-map-argument-positional", "{0.global.theVariable}",
        """[{"global":{"theVariable":"val-from-argument"}}]""", precedence);
    Add("precedence-map-other-key", "{other}",
        """{"global":"dict-value","other":"dict-other"}""", precedence);

    // Registering the source must not change a template that names no
    // variable: a dictionary argument is not an IVariablesGroup, so it falls
    // through this source (rank 2000) to DictionarySource (5000) — behind the
    // list formatter's `{Index}` (4000).
    const string mapItems = """[[{"Index":"X","Name":"a"},{"Index":"Y","Name":"b"}]]""";
    Add("list-index-over-map-items", "{0:list:{Index}|,}", mapItems);
    Add("list-index-over-map-items-with-key", "{0:list:{Name}{Index}|,}", mapItems);
    // The same template without the source registered, which must agree.
    cases.Add(new GoldenCase("var-list-index-no-source", "{0:list:{Index}|,}", mapItems));
    // A variable, on the other hand, is read at this source's rank and wins
    // over the list formatter's `{Index}`, because the group it belongs to is
    // an IVariablesGroup.
    Add("list-index-from-a-group", "{0:list:{global.Index}|,}", """[["a","b"]]""");
    // A group as the current value of a *child format* has no selector to root
    // it, which is how the port tells a group from a map: the variable is
    // still found, but by MapSource (5000) rather than this source (2000), so
    // a name a source in between answers goes to that source instead.
    Add("child-format-of-a-group", "{global:{theVariable}}");
    Add("child-format-of-a-group-shadowed-name", "{global:{Index}}");

    // A group named like a selector another source would answer wins, because
    // the source is ranked ahead of all of them.
    Add("shadow-string-length", "{0.Length.v}", """["abcd"]""", shadowing);
    Add("shadow-string-length-alone", "{0.Length}", """["abcd"]""", shadowing);
}

// ---------------------------------------------------------------------------
// M4: the conditions that read a clock, which the harness pins through
// `SystemTime.SetDateTime` and the port through `SmartSettings::now`.
// ---------------------------------------------------------------------------

static void ClockConditionCases(List<GoldenCase> cases)
{
    var now = PinnedNow();

    void AddDate(string id, string template, DateTime value) =>
        cases.Add(new GoldenCase("conddate-" + id, template, JsonDateTime(value)));

    // Every moment stays within a few hours of the clock or a whole day away
    // from it, so the UTC date .NET compares is the same one the port compares
    // as a civil date whatever the machine's offset is.
    foreach (var (slug, value) in new (string, DateTime)[]
             {
                 ("now", now),
                 ("earlier-today", now.AddHours(-2)),
                 ("later-today", now.AddHours(2)),
                 ("yesterday", now.AddDays(-1)),
                 ("tomorrow", now.AddDays(1)),
                 ("long-past", new DateTime(1111, 1, 1, 1, 1, 1)),
                 ("long-future", new DateTime(5555, 5, 5, 5, 5, 5)),
             })
    {
        // Two parts: the past arm takes the clock itself.
        AddDate($"two-{slug}", "{0:cond:Past|Future}", value);
        // Three: the middle one is today, whatever the time of day.
        AddDate($"three-{slug}", "{0:cond:Past|Today|Future}", value);
        // The formatter auto-detects, so the name is optional.
        AddDate($"auto-{slug}", "{0:Past|Today|Future}", value);
        // More than three parts: the arm is `paramCount - 1`, so the last one
        // is what a date that is neither past nor today takes, and the parts
        // between `Today` and it are unreachable.
        AddDate($"four-{slug}", "{0:cond:Past|Today|Unreachable|Future}", value);
    }
    AddDate("nested-format", "{0:cond:was {:d}|is {:d}|will be {:d}}", now.AddHours(2));
    // A date is not IConvertible for the condition parser, so a complex
    // condition is not a condition at all: the text is written as it stands.
    AddDate("complex-condition", "{0:cond:>0?a|b}", now);
    AddDate("complex-condition-three", "{0:cond:>0?a|b|c}", now.AddDays(1));

    void AddSpan(string id, string template, string span) =>
        cases.Add(new GoldenCase("condts-" + id, template, TimeSpanArgs(span)));

    // A TimeSpan needs no clock: negative / zero / positive, with zero folded
    // into the negative arm unless there are exactly three parts.
    foreach (var (slug, span) in new (string, string)[]
             { ("neg", "-01:00:00"), ("zero", "00:00:00"), ("pos", "01:00:00") })
    {
        AddSpan($"two-{slug}", "{0:cond:overdue|left}", span);
        AddSpan($"three-{slug}", "{0:cond:overdue|due now|left}", span);
        AddSpan($"auto-{slug}", "{0:overdue|due now|left}", span);
        // Four parts: zero folds into the first arm again, and a positive span
        // takes `paramCount - 1`.
        AddSpan($"four-{slug}", "{0:cond:overdue|due now|unreachable|left}", span);
    }
    AddSpan("nested-format", "{0:cond:overdue by {:g}|due now|{:g} left}", "01:00:00");
    // Not IConvertible either, so the condition text is written verbatim.
    AddSpan("complex-condition", "{0:cond:>0?a|b}", "01:00:00");
    AddSpan("complex-condition-three", "{0:cond:>0?a|b|c}", "-01:00:00");
}

/// <summary>
/// The variable groups a case's <see cref="VariableSet"/> asks for, mirrored
/// group for group by <c>variables_fixture</c> in the Rust golden runner.
/// </summary>
static PersistentVariablesSource VariablesFixture(VariableSet set) => set switch
{
    VariableSet.Standard => new PersistentVariablesSource
    {
        {
            "global", new VariablesGroup
            {
                { "theVariable", new StringVariable("persistent-value") },
                { "nested", new VariablesGroup { { "inner", new IntVariable(42) } } },
                { "nullVar", new ObjectVariable(null) },
                // Named like the selector the `list` formatter answers, to
                // show that a group's variable is read before it.
                { "Index", new IntVariable(7) },
            }
        },
        {
            "v", new VariablesGroup
            {
                { "i", new IntVariable(1234) },
                { "b", new BoolVariable(true) },
                { "s", new StringVariable("str") },
                { "dt", new Variable<DateTime>(new DateTime(2024, 12, 31)) },
                { "list", new ObjectVariable(new object?[] { "a", "b", "c" }) },
            }
        },
    },
    VariableSet.Precedence => new PersistentVariablesSource
    {
        {
            "global", new VariablesGroup
            {
                { "theVariable", new StringVariable("val-from-persistent-source") },
            }
        },
    },
    // A group whose name `StringSource` would answer on a string argument.
    VariableSet.Shadowing => new PersistentVariablesSource
    {
        { "Length", new VariablesGroup { { "v", new IntVariable(7) } } },
    },
    _ => throw new InvalidOperationException("unknown variable set: " + set),
};

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
            // A TimeSpan, written in .NET's round-trip ("c") format:
            // `[-][d.]hh:mm:ss[.fffffff]`. The seven fractional digits are
            // exactly the 100 ns tick, so the wire form is lossless.
            if (obj.Count == 1 && obj.TryGetPropertyValue("$ts", out var ts))
                return TimeSpan.ParseExact((string) ts!, "c", CultureInfo.InvariantCulture);
            if (obj.Count == 1 && obj.TryGetPropertyValue("$f", out var f))
                return double.Parse((string) f!, NumberStyles.Float, CultureInfo.InvariantCulture);
            if (obj.Count == 1 && obj.TryGetPropertyValue("$i32", out var i32))
                return int.Parse((string) i32!, NumberStyles.Integer, CultureInfo.InvariantCulture);
            // A value past long.MaxValue, which a JSON number would lose.
            if (obj.Count == 1 && obj.TryGetPropertyValue("$u64", out var u64))
                return ulong.Parse((string) u64!, NumberStyles.Integer, CultureInfo.InvariantCulture);

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

// ulong.MaxValue, which is past long.MaxValue, so the "$u64" marker carries it.
static string JsonUlongMax() => """[{"$u64":"18446744073709551615"}]""";

static string JsonLong(long value) => value.ToString(CultureInfo.InvariantCulture);

// A single string wrapped as the one positional argument of a case.
static string JsonString(string value) => "[" + JsonSerializer.Serialize(value) + "]";

// The clock every case that reads one reads. A method rather than the
// top-level variable because a static local function cannot capture one.
static DateTime PinnedNow() => new(2026, 7, 31, 12, 0, 0, DateTimeKind.Unspecified);

// TimeSpan arguments, each written in .NET's round-trip ("c") format.
static string TimeSpanArgs(params string[] spans) =>
    "[" + string.Join(",", spans.Select(span => $$"""{"$ts":"{{span}}"}""")) + "]";

// A DateTime as the one positional argument of a case.
static string JsonDateTime(DateTime value) =>
    $$"""[{"$dt":"{{value.ToString("O", CultureInfo.InvariantCulture)}}"}]""";

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
    string Id, string Template, string ArgsJson, CaseSettings? Settings = null,
    string Culture = "");

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
    string CustomSelectorChars = "",
    bool ConvertCharacterStringLiterals = true,
    // The M3 extensions that carry configuration of their own. These are not
    // SmartSettings at all — they are properties of a formatter extension —
    // but they select the formatter a case runs with in exactly the same way,
    // so they ride along in the same record and the same JSON object.
    RegexOptions RegexOptions = RegexOptions.None,
    char IsMatchSplitChar = '|',
    string IsMatchPlaceholderName = "m",
    bool IsMatchCanAutoDetect = false,
    SubStringFormatter.SubStringOutOfRangeBehavior SubStringOutOfRangeBehavior =
        SubStringFormatter.SubStringOutOfRangeBehavior.ReturnEmptyString,
    string SubStringNullDisplayString = "",
    char SubStringSplitChar = ',',
    bool SubStringCanAutoDetect = false,
    char IsNullSplitChar = '|',
    bool IsNullCanAutoDetect = false,
    char ListSplitChar = '|',
    bool ListCanAutoDetect = true,
    TemplateSet Templates = TemplateSet.None,
    VariableSet Variables = VariableSet.None,
    LocalizationSet Localization = LocalizationSet.Standard)
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
            Parser =
            {
                ErrorAction = ParseErrorAction,
                ConvertCharacterStringLiterals = ConvertCharacterStringLiterals,
            },
            // The same fixed table for every case — only a `{:L:…}`
            // placeholder ever reaches it — with the two knobs of
            // `LocalizationProvider` a case can ask for.
            Localization = { LocalizationProvider = LocalizationFixture.For(Localization) },
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
        if (ConvertCharacterStringLiterals != Default.ConvertCharacterStringLiterals)
            json["convertCharacterStringLiterals"] = ConvertCharacterStringLiterals;
        if (RegexOptions != Default.RegexOptions)
            json["regexOptions"] = RegexOptions.ToString();
        if (IsMatchSplitChar != Default.IsMatchSplitChar)
            json["isMatchSplitChar"] = IsMatchSplitChar.ToString();
        if (IsMatchPlaceholderName != Default.IsMatchPlaceholderName)
            json["isMatchPlaceholderName"] = IsMatchPlaceholderName;
        if (IsMatchCanAutoDetect != Default.IsMatchCanAutoDetect)
            json["isMatchCanAutoDetect"] = IsMatchCanAutoDetect;
        if (SubStringOutOfRangeBehavior != Default.SubStringOutOfRangeBehavior)
            json["subStringOutOfRangeBehavior"] = SubStringOutOfRangeBehavior.ToString();
        if (SubStringNullDisplayString != Default.SubStringNullDisplayString)
            json["subStringNullDisplayString"] = SubStringNullDisplayString;
        if (SubStringSplitChar != Default.SubStringSplitChar)
            json["subStringSplitChar"] = SubStringSplitChar.ToString();
        if (SubStringCanAutoDetect != Default.SubStringCanAutoDetect)
            json["subStringCanAutoDetect"] = SubStringCanAutoDetect;
        if (IsNullSplitChar != Default.IsNullSplitChar)
            json["isNullSplitChar"] = IsNullSplitChar.ToString();
        if (IsNullCanAutoDetect != Default.IsNullCanAutoDetect)
            json["isNullCanAutoDetect"] = IsNullCanAutoDetect;
        if (ListSplitChar != Default.ListSplitChar)
            json["listSplitChar"] = ListSplitChar.ToString();
        if (ListCanAutoDetect != Default.ListCanAutoDetect)
            json["listCanAutoDetect"] = ListCanAutoDetect;
        if (Templates != Default.Templates)
            json["templates"] = Templates.ToString();
        if (Variables != Default.Variables)
            json["variables"] = Variables.ToString();
        if (Localization != Default.Localization)
            json["localization"] = Localization.ToString();
        return json;
    }
}

/// <summary>
/// The <see cref="ILocalizationProvider"/> every case runs with: a fixed table
/// keyed by culture name, looked up specific culture → parent → invariant.
/// SmartFormat ships a resx-backed provider; this one keeps the table in the
/// harness, where the Rust runner mirrors it entry for entry into a
/// <c>HashMapLocalizationProvider</c>.
///
/// <see cref="FallbackCulture"/> and <see cref="ReturnNameIfNotFound"/> are the
/// two knobs of <c>SmartFormat.Utilities.LocalizationProvider</c>, applied here
/// exactly as its <c>GetString</c> applies them over a single
/// <c>ResourceManager</c>: the requested culture's chain first, then the
/// fallback culture's chain, then the name itself.
/// </summary>
internal sealed class LocalizationFixture : ILocalizationProvider
{
    /// <summary>(culture name, key, translation); <c>""</c> is the invariant culture.</summary>
    public static readonly (string Culture, string Key, string Value)[] Entries =
    [
        ("", "WeTranslateText", "We translate text"),
        ("es", "WeTranslateText", "Traducimos el texto"),
        ("fr", "WeTranslateText", "Nous traduisons des textes"),
        ("de", "WeTranslateText", "Wir übersetzen Text"),
        // A script culture, which `zh-CN` only reaches through
        // `CultureInfo.Parent` — 'zh-CN'.Parent is 'zh-Hans', not 'zh'.
        ("zh-Hans", "WeTranslateText", "我们翻译文本"),
        // A key no culture chain of another culture reaches, so only the
        // fallback culture can find it.
        ("de", "OnlyGerman", "Nur auf Deutsch"),
        ("", "OnlyExistForInvariantCulture", "This entry only exists in the invariant culture resource"),
        ("", "has {:N0} inhabitants", "has {:N0} inhabitants"),
        ("es", "has {:N0} inhabitants", "tiene {:N0} habitantes"),
        ("fr", "has {:N0} inhabitants", "compte {:N0} habitants"),
        ("de", "has {:N0} inhabitants", "hat {:N0} Einwohner"),
        ("", "{0} has {1:N0} inhabitants", "{0} has {1:N0} inhabitants"),
        ("es", "{0} has {1:N0} inhabitants", "{0} tiene {1:N0} habitantes"),
        ("", "{} item", "{} item"),
        ("", "{} items", "{} items"),
        ("es", "{} item", "{} elemento"),
        ("es", "{} items", "{} elementos"),
        ("fr", "{} item", "{} élément"),
        ("fr", "{} items", "{} éléments"),
        ("de", "{} item", "{} Element"),
        ("de", "{} items", "{} Elemente"),
        ("", "greet", "Hello, {Name}!"),
        ("de", "greet", "Hallo, {Name}!"),
        // A translation that localizes again.
        ("", "Outer", "<{:L:Inner}>"),
        ("", "Inner", "INNER"),
        // Translations that are found and then fail while being rendered: the
        // inner key has no translation, and the selector has no source.
        ("", "OuterMissingInner", "<{:L:NoSuchInner}>"),
        ("", "greetNobody", "Hello {Nope}!"),
        // The key an escape sequence in the format resolves to.
        ("", "a{b", "escaped"),
        // A translation that does not parse.
        ("", "BadParse", "{0:"),
        // Two translations that differ only in case, which collide in the
        // parse cache when the settings ignore case.
        ("", "K1", "abc {0}"),
        ("", "K2", "ABC {0}"),
    ];

    /// <summary>The fallback culture of <see cref="LocalizationSet.Fallback"/>.</summary>
    public const string FallbackCultureName = "de";

    // Declared after the table it reads: static fields are initialized in
    // declaration order.
    public static readonly LocalizationFixture Instance = new(null, false);

    private static readonly LocalizationFixture WithFallbackCulture =
        new(CultureInfo.GetCultureInfo(FallbackCultureName), false);

    private static readonly LocalizationFixture ReturningTheName = new(null, true);

    public static LocalizationFixture For(LocalizationSet set) => set switch
    {
        LocalizationSet.Standard => Instance,
        LocalizationSet.Fallback => WithFallbackCulture,
        LocalizationSet.ReturnName => ReturningTheName,
        _ => throw new InvalidOperationException("unknown localization set: " + set),
    };

    private readonly Dictionary<string, Dictionary<string, string>> _tables;
    private readonly CultureInfo? _fallbackCulture;
    private readonly bool _returnNameIfNotFound;

    private LocalizationFixture(CultureInfo? fallbackCulture, bool returnNameIfNotFound)
    {
        _fallbackCulture = fallbackCulture;
        _returnNameIfNotFound = returnNameIfNotFound;
        _tables = new Dictionary<string, Dictionary<string, string>>(StringComparer.Ordinal);
        foreach (var (culture, key, value) in Entries)
        {
            if (!_tables.TryGetValue(culture, out var table))
                _tables[culture] = table = new Dictionary<string, string>(StringComparer.Ordinal);
            table.Add(key, value);
        }
    }

    public string? GetString(string name) => Lookup(name, CultureInfo.CurrentUICulture);

    public string? GetString(string name, string cultureName) =>
        Lookup(name, CultureInfo.GetCultureInfo(cultureName));

    public string? GetString(string name, CultureInfo cultureInfo) => Lookup(name, cultureInfo);

    // `LocalizationProvider.GetString`, for one resource: the culture's own
    // chain, then the fallback culture's, then the name itself.
    private string? Lookup(string name, CultureInfo culture)
    {
        var value = WalkChain(name, culture);
        if (value is null && _fallbackCulture != null) value = WalkChain(name, _fallbackCulture);
        if (value is null && _returnNameIfNotFound) return name;
        return value;
    }

    private string? WalkChain(string name, CultureInfo culture)
    {
        for (var c = culture;; c = c.Parent)
        {
            if (_tables.TryGetValue(c.Name, out var table) && table.TryGetValue(name, out var value))
                return value;
            if (c.Equals(CultureInfo.InvariantCulture)) return null;
        }
    }
}

/// <summary>
/// How the <c>ILocalizationProvider</c> a case runs with is configured. The
/// table is the same either way; what changes are the two knobs
/// <c>SmartFormat.Utilities.LocalizationProvider</c> carries.
/// </summary>
internal enum LocalizationSet
{
    /// <summary>No fallback culture, a miss is a miss.</summary>
    Standard,
    /// <summary>A fallback culture, walked when the requested chain misses.</summary>
    Fallback,
    /// <summary>A miss answers with the requested name.</summary>
    ReturnName,
}

/// <summary>Which set of named templates a case has registered, if any.</summary>
internal enum TemplateSet
{
    None,
    Standard,
    WithEmptyName,
    CaseInsensitive,
    Simple,
}

/// <summary>
/// Which set of persistent variable groups a case has registered, if any. The
/// three sets are separate because a group name wins over every source in the
/// default registry, so one fixture holding all of them would answer selectors
/// the other cases need an argument to answer.
/// </summary>
internal enum VariableSet
{
    None,
    /// <summary>The groups the `var-*` cases read: `global` and `v`.</summary>
    Standard,
    /// <summary>One group, whose variable an argument of the same shape shadows.</summary>
    Precedence,
    /// <summary>A group named `Length`, which `StringSource` would answer.</summary>
    Shadowing,
}
