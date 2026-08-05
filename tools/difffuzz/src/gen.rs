//! The grammar-driven generator, and the syntax tree it generates into.
//!
//! Uniformly random text is worthless here: almost none of it parses, so the
//! whole campaign would exercise one error path. Everything below builds a
//! template out of weighted *constructs* instead — literals with escapes,
//! placeholders with real selectors, alignment, formatter names with options,
//! and the split-based formatters with realistic and deliberately wrong part
//! counts — together with an argument tree the selectors mostly resolve
//! against, so the rendering path is reached and not just the reporting one.
//!
//! Weights lean towards what `goldens/m1.json` is thin on: placeholders nested
//! inside split parts, alignment combined with formatter options, escapes
//! inside options, cultures other than the invariant one, and the three
//! `ErrorAction`s that recover rather than throw.

use serde_json::{json, Map, Value as Json};

use crate::case::Case;
use crate::rng::Rng;

// ---------------------------------------------------------------------------
// The syntax tree
// ---------------------------------------------------------------------------

/// A generated template. Keeping the tree, rather than only the text it
/// renders to, is what lets the shrinker drop an item or an option instead of
/// cutting characters and hoping the result still parses.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Template {
    pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    /// Literal text, exactly as it is written into the template — escapes
    /// included, well-formed or not.
    Literal(String),
    Placeholder(Box<Placeholder>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Placeholder {
    /// The selector text between `{` and the first `,` or `:`; empty is the
    /// nameless `{}`.
    pub selector: String,
    pub alignment: Option<i64>,
    pub format: Option<FormatSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FormatSpec {
    /// The formatter name, or empty for a bare format such as `{0:N2}`.
    pub name: String,
    /// The raw text between `(` and `)`, escapes included.
    pub options: Option<String>,
    /// The format, split on `|`. One part means no split at all.
    pub parts: Vec<Vec<Node>>,
}

impl Template {
    pub fn render(&self) -> String {
        let mut out = String::new();
        render_nodes(&self.nodes, &mut out);
        out
    }
}

fn render_nodes(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Literal(text) => out.push_str(text),
            Node::Placeholder(placeholder) => render_placeholder(placeholder, out),
        }
    }
}

fn render_placeholder(placeholder: &Placeholder, out: &mut String) {
    out.push('{');
    out.push_str(&placeholder.selector);
    if let Some(alignment) = placeholder.alignment {
        out.push(',');
        out.push_str(&alignment.to_string());
    }
    if let Some(format) = &placeholder.format {
        out.push(':');
        // A name (or options) needs the second colon; without one the whole
        // remainder is the format, which is how `{0:N2}` is written.
        if !format.name.is_empty() || format.options.is_some() {
            out.push_str(&format.name);
            if let Some(options) = &format.options {
                out.push('(');
                out.push_str(options);
                out.push(')');
            }
            out.push(':');
        }
        for (index, part) in format.parts.iter().enumerate() {
            if index > 0 {
                out.push('|');
            }
            render_nodes(part, out);
        }
    }
    out.push('}');
}

// ---------------------------------------------------------------------------
// The argument tree
// ---------------------------------------------------------------------------

/// What a selector path resolves to, so the generator can put a number where a
/// number is wanted and a list where a list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Integer,
    Float,
    Text,
    Bool,
    List,
    Map,
    Null,
    Date,
    Span,
}

/// The argument tree, with the selector paths that resolve against it.
pub struct Env {
    pub args: Json,
    paths: Vec<(Kind, String)>,
}

impl Env {
    fn add(&mut self, kind: Kind, path: impl Into<String>) {
        self.paths.push((kind, path.into()));
    }

    /// A path of one of the given kinds, or `None` when the tree grew none.
    fn path_of(&self, rng: &mut Rng, kinds: &[Kind]) -> Option<String> {
        let matching: Vec<&String> = self
            .paths
            .iter()
            .filter(|(kind, _)| kinds.contains(kind))
            .map(|(_, path)| path)
            .collect();
        if matching.is_empty() {
            return None;
        }
        Some(rng.pick(&matching).to_string())
    }

    fn any_path(&self, rng: &mut Rng) -> String {
        rng.pick(&self.paths).1.clone()
    }
}

/// A JSON number that the harness's `IsIntegerLiteral` reads back as a double:
/// Rust's `{:?}` always writes a `.` or an exponent for a finite float, and the
/// non-finite ones have no JSON form at all, so they take the `$f` marker.
fn float(value: f64) -> Json {
    if !value.is_finite() {
        let text = if value.is_nan() {
            "NaN"
        } else if value > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        };
        return json!({ "$f": text });
    }
    let text = format!("{value:?}");
    serde_json::from_str(&text).expect("a finite float is a JSON number")
}

fn date(rng: &mut Rng) -> Json {
    let samples = [
        "2024-03-05T14:30:45.1234567",
        "1999-12-31T23:59:59.9990000",
        "2026-07-31T12:00:00.0000000",
        "2026-07-31T11:59:59.0000000",
        "2026-08-01T00:00:00.0000000",
        "0001-01-01T00:00:00.0000000",
        "2024-02-29T00:00:00.0000000",
    ];
    json!({ "$dt": rng.pick(&samples) })
}

fn span(rng: &mut Rng) -> Json {
    let samples = [
        "3.04:05:06.0000000",
        "00:00:00.0000000",
        "-1.02:03:04.5000000",
        "00:01:30.0000000",
        "400.00:00:00.0000000",
    ];
    json!({ "$ts": rng.pick(&samples) })
}

fn text(rng: &mut Rng) -> Json {
    let samples = [
        "hello world",
        "Straße",
        "ǆungla",
        "  padded  ",
        "",
        "ÅNGSTRÖM",
        "日本語のテキスト",
        "a|b",
        "line\nbreak",
        "İstanbul",
        "42",
        "-7.5",
    ];
    json!(rng.pick(&samples))
}

/// One entry of the map the argument tree is built around. Kinds are fixed per
/// key so a template that reads `Qty` always reads a number.
///
/// No name here may be a member of the CLR type the harness maps a JSON object
/// to. `args` becomes a `Dictionary<string, object>`, whose `Count`, `Keys`,
/// `Values` and `Comparer` .NET's `ReflectionSource` will happily resolve when
/// the map has no such key — so `{Count}` on a map without one answers the
/// dictionary's *entry count* in .NET and nothing at all here, where a
/// `Value::Map` has no members beyond its keys. That is the harness's choice of
/// representation showing through, not a difference in the port, so the field
/// is `Qty` rather than `Count`.
const FIELDS: [(&str, Kind); 11] = [
    ("Name", Kind::Text),
    ("City", Kind::Text),
    ("Qty", Kind::Integer),
    ("Price", Kind::Float),
    ("Items", Kind::List),
    ("When", Kind::Date),
    ("Span", Kind::Span),
    ("Flag", Kind::Bool),
    ("Nothing", Kind::Null),
    ("Nested", Kind::Map),
    ("Text", Kind::Text),
];

fn value_of(rng: &mut Rng, kind: Kind) -> Json {
    match kind {
        Kind::Integer => json!(rng.weighted_pick(&[
            (30, 1i64),
            (30, 0),
            (20, 2),
            (10, -3),
            (5, 1_000_000),
            (5, i64::MIN),
        ])),
        Kind::Float => float(*rng.weighted_pick(&[
            (30, 1.5f64),
            (20, 0.0),
            (15, -2.25),
            (10, 1234567.891),
            (10, 1e300),
            (5, f64::NAN),
            (5, f64::INFINITY),
            (5, 1e-9),
        ])),
        Kind::Text => text(rng),
        Kind::Bool => json!(rng.chance(50)),
        Kind::List => {
            let length = rng.below(4);
            Json::Array(
                (0..length)
                    .map(|index| {
                        if rng.chance(50) {
                            json!(index as i64 + 1)
                        } else {
                            text(rng)
                        }
                    })
                    .collect(),
            )
        }
        Kind::Map => json!({ "Inner": rng.range(0, 9), "Deep": text(rng) }),
        Kind::Null => Json::Null,
        Kind::Date => date(rng),
        Kind::Span => span(rng),
    }
}

/// Builds the argument tree and the paths into it. The root is either a single
/// map — `{Name}` then resolves straight off it — or a positional array, where
/// the same field is reached as `{0.Name}`; the harness maps the two to a
/// `Dictionary<string, object>` and an `object[]` respectively.
fn build_env(rng: &mut Rng, features: &Features) -> Env {
    let field_count = rng.range(3, FIELDS.len() as i64) as usize;
    let mut chosen: Vec<(&str, Kind)> = FIELDS.to_vec();
    while chosen.len() > field_count {
        let index = rng.below(chosen.len());
        chosen.remove(index);
    }

    let mut map = Map::new();
    for (name, kind) in &chosen {
        map.insert((*name).to_string(), value_of(rng, *kind));
    }

    let root_is_map = rng.chance(45);
    let mut env = Env {
        args: Json::Null,
        paths: Vec::new(),
    };

    let prefix = if root_is_map {
        env.args = Json::Object(map);
        env.add(Kind::Map, "0");
        String::new()
    } else {
        let extra = [
            value_of(rng, Kind::Integer),
            value_of(rng, Kind::Text),
            value_of(rng, Kind::List),
        ];
        env.args = json!([Json::Object(map), extra[0], extra[1], extra[2]]);
        env.add(Kind::Map, "0");
        env.add(Kind::Integer, "1");
        env.add(Kind::Text, "2");
        env.add(Kind::List, "3");
        "0.".to_string()
    };

    for (name, kind) in &chosen {
        let path = format!("{prefix}{name}");
        env.add(*kind, path.clone());
        match kind {
            Kind::Text => {
                if rng.chance(60) {
                    env.add(Kind::Text, format!("{path}.ToUpper"));
                }
                if rng.chance(40) {
                    env.add(Kind::Text, format!("{path}.ToLower"));
                }
                if rng.chance(30) {
                    env.add(Kind::Integer, format!("{path}.Length"));
                }
                if rng.chance(20) {
                    let method = rng.pick(&["Trim", "Capitalize", "CapitalizeWords"]);
                    env.add(Kind::Text, format!("{path}.{method}"));
                }
            }
            Kind::Map => {
                env.add(Kind::Integer, format!("{path}.Inner"));
                env.add(Kind::Text, format!("{path}.Deep"));
                if rng.chance(30) {
                    // A nullable selector on a value that is never null: the
                    // `?.` short-circuit has to stay out of the way.
                    env.add(Kind::Integer, format!("{path}?.Inner"));
                }
            }
            Kind::List => {
                if rng.chance(50) {
                    env.add(Kind::Integer, format!("{path}[0]"));
                }
            }
            // A nullable selector on a null: `?.` short-circuits the rest of
            // the chain rather than failing on it.
            Kind::Null if rng.chance(40) => env.add(Kind::Null, format!("{path}?.Anything")),
            _ => {}
        }
    }

    // The persistent-variables fixture the golden runner registers, reachable
    // only when the case asked for it.
    if features.variables {
        for (kind, path) in [
            (Kind::Text, "global.theVariable"),
            (Kind::Integer, "global.nested.inner"),
            (Kind::Null, "global.nullVar"),
            (Kind::Integer, "global.Index"),
            (Kind::Integer, "v.i"),
            (Kind::Bool, "v.b"),
            (Kind::Text, "v.s"),
            (Kind::Date, "v.dt"),
            (Kind::List, "v.list"),
        ] {
            env.add(kind, path);
        }
    }

    env
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// What a case's settings make available to the template generator.
pub struct Features {
    pub templates: bool,
    pub variables: bool,
    /// `StringFormatCompatibility` turns off nesting, `|` splitting and the
    /// nameless placeholder, so the generator stops emitting them.
    pub string_format_compatibility: bool,
}

const CULTURES: [&str; 20] = [
    "de", "de-DE", "de-CH", "fr", "fr-FR", "es", "es-MX", "ru", "ja", "zh-CN", "is-IS", "tr",
    "pt-BR", "en-GB", "en-US", "nl", "pl", "sv", "fi", "cs",
];

fn generate_settings(rng: &mut Rng) -> (Map<String, Json>, Features) {
    let mut settings = Map::new();

    // Deliberately heavy: the golden corpus runs almost everything under the
    // throwing action, so the recovering ones are where a difference hides.
    // They also turn an exception — of which .NET only tells us the type name —
    // into text, which is what can actually be diffed byte for byte.
    let actions = ["Ignore", "MaintainTokens", "OutputErrorInResult"];
    if rng.chance(55) {
        settings.insert(
            "formatErrorAction".into(),
            json!(rng.pick(&actions).to_string()),
        );
    }
    if rng.chance(45) {
        settings.insert(
            "parseErrorAction".into(),
            json!(rng.pick(&actions).to_string()),
        );
    }

    let case_insensitive = rng.chance(12);
    if case_insensitive {
        settings.insert("caseSensitivity".into(), json!("CaseInsensitive"));
    }
    let string_format_compatibility = rng.chance(4);
    if string_format_compatibility {
        settings.insert("stringFormatCompatibility".into(), json!(true));
    }
    if rng.chance(10) {
        settings.insert(
            "alignmentFillCharacter".into(),
            json!(rng.pick(&[".", "0", "*", "\u{00b7}"]).to_string()),
        );
    }
    if rng.chance(3) {
        // Only characters SmartFormat gives no meaning of its own, so adding
        // them can never be rejected by either side.
        settings.insert(
            "customSelectorChars".into(),
            json!(rng.pick(&["#", "@", "#@"]).to_string()),
        );
    }
    if rng.chance(6) {
        settings.insert("convertCharacterStringLiterals".into(), json!(false));
    }
    if rng.chance(10) {
        settings.insert(
            "regexOptions".into(),
            json!(rng
                .pick(&["IgnoreCase", "Multiline", "Singleline", "CultureInvariant"])
                .to_string()),
        );
    }
    if rng.chance(8) {
        settings.insert(
            "subStringOutOfRangeBehavior".into(),
            json!(rng
                .pick(&["ReturnStartIndexToEndOfString", "ThrowException"])
                .to_string()),
        );
    }
    if rng.chance(5) {
        settings.insert("subStringNullDisplayString".into(), json!("(none)"));
    }
    if rng.chance(4) {
        settings.insert("listCanAutoDetect".into(), json!(false));
    }
    if rng.chance(4) {
        settings.insert("isNullCanAutoDetect".into(), json!(true));
    }
    if rng.chance(4) {
        settings.insert("subStringCanAutoDetect".into(), json!(true));
    }
    if rng.chance(6) {
        settings.insert(
            "localization".into(),
            json!(rng.pick(&["Fallback", "ReturnName"]).to_string()),
        );
    }

    let variables = rng.chance(18);
    if variables {
        settings.insert("variables".into(), json!("Standard"));
    }

    // Two combinations the harness cannot build at all, because .NET's
    // `Dictionary.Add` throws while the fixture is being registered: the
    // standard set does not parse under `StringFormatCompatibility`, and
    // `LAST` collides with `last` under an ordinal-ignore-case comparer.
    let templates = rng.chance(18);
    if templates {
        let set = if string_format_compatibility {
            "Simple"
        } else if case_insensitive {
            "CaseInsensitive"
        } else {
            "Standard"
        };
        settings.insert("templates".into(), json!(set));
    }

    (
        settings,
        Features {
            templates,
            variables,
            string_format_compatibility,
        },
    )
}

// ---------------------------------------------------------------------------
// Template generation
// ---------------------------------------------------------------------------

/// Literal fragments, weighted. The escape shapes and the malformed ones next
/// to them are here rather than in a separate pass because that is where they
/// occur in real templates: in the middle of ordinary text.
fn literal(rng: &mut Rng) -> String {
    let table: &[(u32, &str)] = &[
        (60, "x"),
        (40, " "),
        (30, "abc"),
        (25, ", "),
        (20, "Hello, "),
        (15, ": "),
        (12, "-"),
        (10, "Ünïcødé"),
        (8, "日本"),
        (6, "\u{1f642}"),
        // Well-formed escapes.
        (25, r"\{"),
        (25, r"\}"),
        (20, r"\\"),
        (18, r"\n"),
        (12, r"\t"),
        (10, r"\r"),
        (14, r"A"),
        (10, r"é"),
        (8, r"中"),
        // The malformed shapes that live next to them.
        (8, r"\q"),
        (6, r"\u12"),
        (6, r"\uZZZZ"),
        (5, r"\u"),
        // A lone surrogate half: .NET keeps a UTF-16 code unit here and Rust
        // cannot, which is a documented divergence the classifier knows.
        (4, r"\ud83d"),
        (4, r"🙂"),
        // Unbalanced braces, which are parse errors rather than escapes.
        (6, "}"),
        (3, "{"),
    ];
    rng.weighted_pick(table).to_string()
}

/// A backslash at the very end of a template is its own error: escape
/// resolution runs off the end of the string.
fn trailing_escape(rng: &mut Rng) -> Option<String> {
    if rng.chance(3) {
        Some("\\".to_string())
    } else {
        None
    }
}

/// The formatter names the default registry answers to, plus the two the
/// golden runner adds and a few that resolve to nothing on purpose.
fn formatter_name(rng: &mut Rng, features: &Features) -> String {
    let mut table: Vec<(u32, &str)> = vec![
        (55, ""), // a bare format: `{0:N2}`
        (30, "list"),
        (25, "plural"),
        (25, "cond"),
        (22, "choose"),
        (18, "isnull"),
        (18, "ismatch"),
        (20, "substr"),
        (12, "time"),
        (10, "L"),
        (8, "d"),
        // Names nothing claims, which is a "no suitable formatter" report on
        // one side and has to be the same one on the other.
        (6, "nope"),
        (4, "List"),
        (3, "PLURAL"),
    ];
    if features.templates {
        table.push((20, "t"));
    }
    rng.weighted_pick(&table).to_string()
}

/// The raw text of a formatter's options, per formatter. Escapes inside options
/// are over-weighted on purpose: `\)`, `\|` and `\\` there are a corner the
/// golden corpus barely touches.
fn options(rng: &mut Rng, name: &str) -> String {
    let generic: &[(u32, &str)] = &[
        (10, ""),
        (8, "1"),
        (6, "a|b"),
        (6, r"a\|b"),
        (6, r"a\)b"),
        (4, r"\\"),
        (3, "("),
        (3, ","),
    ];
    let table: &[(u32, &str)] = match name {
        "choose" => &[
            (25, "1|2|3"),
            (20, "a|b"),
            (15, "true|false"),
            (10, "null"),
            (10, "0|1|2|3"),
            (8, r"a\|b|c"),
            (6, r"a\)b|c"),
            (6, ""),
        ],
        "ismatch" => &[
            (20, "^a"),
            (18, r"\d+"),
            (15, "[a-z]+"),
            (10, "(a)(b)"),
            (10, "a|b"),
            (8, "(?i)x"),
            (6, r"\w\b"),
            (6, "["),
            (5, r"a\)b"),
        ],
        "substr" => &[
            (25, "0,3"),
            (20, "2"),
            (15, "-3"),
            (12, "1,99"),
            (10, "0,0"),
            (8, "-99,2"),
            (6, ","),
            (5, "abc"),
        ],
        "plural" => &[
            (30, "en"),
            (20, "de"),
            (15, "ru"),
            (10, "is"),
            (8, "zz"),
            (8, ""),
            (6, "en-US"),
        ],
        "time" => &[
            (20, "hours minutes"),
            (15, "abbr"),
            (12, "noless"),
            (12, "w2"),
            (10, "auto"),
            (10, "short"),
            (8, "fill"),
            (8, "en"),
            (6, "seconds"),
        ],
        "L" => &[(30, "de"), (20, "fr"), (15, "es"), (10, "zz"), (8, "")],
        _ => generic,
    };
    rng.weighted_pick(table).to_string()
}

/// How many parts the format is split into, per formatter: the counts a real
/// template uses, and the counts that are wrong on purpose.
fn part_count(rng: &mut Rng, name: &str) -> usize {
    let table: &[(u32, usize)] = match name {
        "list" => &[(35, 2), (30, 3), (15, 4), (10, 1), (5, 5), (5, 0)],
        "plural" => &[(40, 2), (25, 3), (15, 1), (10, 4), (10, 6)],
        "cond" => &[(35, 2), (30, 3), (15, 1), (10, 4), (10, 6)],
        "choose" => &[(30, 2), (25, 3), (20, 4), (15, 1), (10, 5)],
        "isnull" => &[(55, 2), (25, 1), (10, 3), (10, 0)],
        "ismatch" => &[(55, 2), (20, 1), (15, 3), (10, 0)],
        "substr" | "time" | "t" | "L" | "d" => &[(75, 1), (15, 2), (10, 0)],
        // A bare format: splitting it is what the auto-detecting formatters
        // pick up, which is the whole point of the `|` weights here.
        "" => &[(55, 1), (25, 2), (12, 3), (8, 4)],
        _ => &[(50, 1), (25, 2), (15, 3), (10, 0)],
    };
    *rng.weighted_pick(table)
}

/// A selector, mostly one the argument tree answers.
fn selector(rng: &mut Rng, env: &Env, features: &Features, inside_part: bool) -> String {
    // Inside a split part the nameless placeholder is the item being
    // formatted, which is how `list` and `plural` templates are actually
    // written — and it is meaningless under StringFormatCompatibility.
    if inside_part && !features.string_format_compatibility && rng.chance(35) {
        return String::new();
    }
    if inside_part && rng.chance(10) {
        return "Index".to_string();
    }
    match rng.weighted(&[80, 8, 4, 4, 4]) {
        0 => env.any_path(rng),
        // A name nothing resolves.
        1 => rng.pick(&["Missing", "nope.deep", "99", "-1"]).to_string(),
        // The nameless placeholder outside a list, where it is the whole
        // argument set.
        2 if !features.string_format_compatibility => String::new(),
        // A selector chain one step longer than the tree is deep.
        3 => format!("{}.More", env.any_path(rng)),
        _ => env.any_path(rng),
    }
}

/// A bare format — the `N2` of `{0:N2}` — chosen for what the selector holds.
fn bare_format(rng: &mut Rng, kind: Option<Kind>) -> String {
    let numeric: &[(u32, &str)] = &[
        (20, "N2"),
        (15, "N0"),
        (12, "D3"),
        (12, "0.00"),
        (10, "X"),
        (10, "P1"),
        (8, "e2"),
        (8, "F3"),
        (8, "C"),
        (6, "G"),
        (5, "#,##0.###"),
        (4, "B"),
    ];
    let dates: &[(u32, &str)] = &[
        (20, "d"),
        (15, "D"),
        (12, "yyyy-MM-dd"),
        (12, "t"),
        (10, "HH:mm:ss"),
        (10, "o"),
        (8, "f"),
        (8, "M"),
        (5, "U"),
    ];
    let generic: &[(u32, &str)] = &[
        (30, "N2"),
        (20, "d"),
        (15, ""),
        (10, "yyyy"),
        (10, "X"),
        (8, "zz"),
        (7, "0.0"),
    ];
    let table = match kind {
        Some(Kind::Integer | Kind::Float) => numeric,
        Some(Kind::Date | Kind::Span) => dates,
        _ => generic,
    };
    rng.weighted_pick(table).to_string()
}

struct Ctx<'a> {
    env: &'a Env,
    features: &'a Features,
}

/// A run of nodes. `depth` bounds nesting; `inside_part` says whether these
/// nodes sit inside a split part, which is where nesting is most interesting
/// and least covered by the golden corpus.
fn nodes(rng: &mut Rng, ctx: &Ctx<'_>, depth: u32, inside_part: bool) -> Vec<Node> {
    // Each level multiplies out — up to five parts, each a run of nodes, each
    // able to hold another placeholder — so the run gets shorter as it gets
    // deeper. Without that the tail of the length distribution runs to
    // thousands of characters, which costs the harness time and buys nothing
    // the shrinker would not have thrown away.
    let count = match (inside_part, depth) {
        (false, _) => rng.range(1, 6) as usize,
        (true, 0 | 1) => rng.range(1, 3) as usize,
        (true, _) => rng.range(1, 2) as usize,
    };
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        // Deep inside a nested format a literal keeps the template readable;
        // near the top a placeholder is what carries the interest.
        let placeholder_chance = if depth == 0 {
            55
        } else if inside_part {
            60
        } else {
            35
        };
        if depth < 3 && rng.chance(placeholder_chance) {
            out.push(Node::Placeholder(Box::new(placeholder(
                rng,
                ctx,
                depth,
                inside_part,
            ))));
        } else {
            out.push(Node::Literal(literal(rng)));
        }
    }
    out
}

fn placeholder(rng: &mut Rng, ctx: &Ctx<'_>, depth: u32, inside_part: bool) -> Placeholder {
    let name = if rng.chance(58) {
        formatter_name(rng, ctx.features)
    } else {
        String::new()
    };

    // The formatter is picked first so the selector can be picked to suit it:
    // a `list` on a list and a `substr` on a string reach the formatting code,
    // where a `list` on an integer only ever reaches the "cannot" branch. The
    // wrong pairing is worth generating too, which is what the other 30% is.
    let wanted: &[Kind] = match name.as_str() {
        "list" => &[Kind::List],
        "plural" => &[Kind::Integer, Kind::Float, Kind::List],
        "cond" => &[
            Kind::Integer,
            Kind::Float,
            Kind::Bool,
            Kind::Null,
            Kind::Date,
        ],
        "choose" => &[Kind::Integer, Kind::Text, Kind::Bool, Kind::Null],
        "isnull" => &[Kind::Null, Kind::Text, Kind::Integer],
        "ismatch" | "substr" => &[Kind::Text],
        // A `TimeSpan` only, never a `DateTime`. `TimeFormatter` on a
        // `DateTime` subtracts it from the clock, and .NET subtracts the two
        // *UTC* moments where a zone-less `jiff::civil::DateTime` has nothing
        // to convert with — so the answers differ by exactly the daylight-saving
        // offset between the two moments, and only on a machine whose zone has
        // a transition between them. `tools/goldens/README.md` states the rule
        // ("keep `now` and any nearby value on the same side of a daylight-saving
        // transition, which is why `now` is in July") and nothing checks it; a
        // generator that broke it would report the same finding on one machine
        // and not on the next, which is worse than not generating it. A
        // `TimeSpan` reads no clock, so it is safe everywhere.
        "time" => &[Kind::Span],
        // A bare format, or none at all. Default-formatting a list or a map is
        // a divergence `DESIGN.md` already records — .NET writes the CLR type
        // name — so leaning away from those keeps the campaign's output from
        // filling up with a difference nobody has to look at again.
        "" | "d" => &[
            Kind::Integer,
            Kind::Float,
            Kind::Text,
            Kind::Bool,
            Kind::Date,
            Kind::Span,
            Kind::Null,
        ],
        _ => &[],
    };
    let biased = !wanted.is_empty() && rng.chance(70) && (!inside_part || rng.chance(50));
    let selector = biased
        .then(|| ctx.env.path_of(rng, wanted))
        .flatten()
        .unwrap_or_else(|| selector(rng, ctx.env, ctx.features, inside_part));

    let kind = ctx
        .env
        .paths
        .iter()
        .find(|(_, path)| *path == selector)
        .map(|(kind, _)| *kind);

    let wants_format = rng.chance(70) || !name.is_empty();

    let format = if !wants_format {
        None
    } else if name.is_empty() {
        // A bare format: no name, no options, and only a `|` split when
        // something is meant to auto-detect it.
        let count = part_count(rng, "");
        let parts = if count <= 1 || ctx.features.string_format_compatibility {
            vec![vec![Node::Literal(bare_format(rng, kind))]]
        } else {
            (0..count)
                .map(|_| nodes(rng, ctx, depth + 1, true))
                .collect()
        };
        Some(FormatSpec {
            name: String::new(),
            options: None,
            parts,
        })
    } else {
        let with_options = matches!(
            name.as_str(),
            "choose" | "ismatch" | "substr" | "plural" | "time" | "L"
        );
        // Escapes inside options and alignment beside them are the two thin
        // spots the task names, so options are common on the formatters that
        // take them and appear even on those that do not.
        let options = if with_options && rng.chance(80) {
            Some(options(rng, &name))
        } else if rng.chance(8) {
            Some(options(rng, "generic"))
        } else {
            None
        };
        let count = if ctx.features.string_format_compatibility {
            1
        } else {
            part_count(rng, &name)
        };
        let parts = (0..count.max(1))
            .map(|_| {
                if count == 0 {
                    Vec::new()
                } else {
                    nodes(rng, ctx, depth + 1, true)
                }
            })
            .collect();
        Some(FormatSpec {
            name,
            options,
            parts,
        })
    };

    // Alignment is far more likely once there are options to interact with:
    // where the padding is applied relative to a formatter's own output is
    // exactly the interaction the corpus is thin on.
    let has_options = format.as_ref().is_some_and(|f| f.options.is_some());
    let alignment_chance = if has_options { 45 } else { 18 };
    let alignment = if rng.chance(alignment_chance) {
        Some(*rng.weighted_pick(&[
            (25, 5i64),
            (25, -5),
            (15, 1),
            (15, -1),
            (10, 12),
            (5, -12),
            (5, 0),
        ]))
    } else {
        None
    };

    Placeholder {
        selector,
        alignment,
        format,
    }
}

/// Builds case `index` of the campaign seeded with `seed`. The stream is
/// derived from `(seed, index)` rather than drawn in sequence, so a case is
/// reproducible on its own — `--seed S --index N` rebuilds exactly this one.
pub fn generate(seed: u64, index: usize) -> Case {
    let rng = &mut Rng::derive(seed, index as u64);

    let (settings, features) = generate_settings(rng);
    let env = build_env(rng, &features);
    let ctx = Ctx {
        env: &env,
        features: &features,
    };

    let mut tree = Template {
        nodes: nodes(rng, &ctx, 0, false),
    };
    if let Some(tail) = trailing_escape(rng) {
        tree.nodes.push(Node::Literal(tail));
    }

    // Half the campaign runs somewhere other than the invariant culture: that
    // is where a separator, a sign or a month name can differ, and the golden
    // corpus only reaches it through a handful of hand-written cases.
    let culture = if rng.chance(50) {
        (*rng.pick(&CULTURES)).to_string()
    } else {
        String::new()
    };

    Case {
        id: format!("fz-{seed}-{index}"),
        tree,
        args: env.args,
        culture,
        settings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_and_index_reproduce_a_case() {
        for index in 0..50 {
            let first = generate(4242, index);
            let second = generate(4242, index);
            assert_eq!(first.template(), second.template());
            assert_eq!(first.args, second.args);
            assert_eq!(first.culture, second.culture);
            assert_eq!(first.settings, second.settings);
        }
    }

    #[test]
    fn cases_of_one_campaign_differ() {
        let templates: std::collections::HashSet<String> = (0..200)
            .map(|index| generate(9, index).template())
            .collect();
        assert!(
            templates.len() > 150,
            "only {} distinct templates in 200 cases",
            templates.len()
        );
    }

    #[test]
    fn rendering_round_trips_through_the_tree() {
        let tree = Template {
            nodes: vec![
                Node::Literal("a".into()),
                Node::Placeholder(Box::new(Placeholder {
                    selector: "0".into(),
                    alignment: Some(-5),
                    format: Some(FormatSpec {
                        name: "plural".into(),
                        options: Some("en".into()),
                        parts: vec![
                            vec![Node::Literal("one".into())],
                            vec![Node::Literal("many".into())],
                        ],
                    }),
                })),
            ],
        };
        assert_eq!(tree.render(), "a{0,-5:plural(en):one|many}");
    }

    #[test]
    fn a_bare_format_writes_one_colon() {
        let tree = Template {
            nodes: vec![Node::Placeholder(Box::new(Placeholder {
                selector: "0".into(),
                alignment: None,
                format: Some(FormatSpec {
                    name: String::new(),
                    options: None,
                    parts: vec![vec![Node::Literal("N2".into())]],
                }),
            }))],
        };
        assert_eq!(tree.render(), "{0:N2}");
    }

    #[test]
    fn the_nameless_placeholder_writes_empty_braces() {
        let tree = Template {
            nodes: vec![Node::Placeholder(Box::new(Placeholder {
                selector: String::new(),
                alignment: None,
                format: None,
            }))],
        };
        assert_eq!(tree.render(), "{}");
    }

    #[test]
    fn incompatible_settings_are_never_combined() {
        for seed in 0..400u64 {
            let case = generate(seed, 0);
            let compatibility = case.settings.get("stringFormatCompatibility");
            let sensitivity = case.settings.get("caseSensitivity");
            if let Some(set) = case.settings.get("templates").and_then(Json::as_str) {
                if compatibility.is_some() {
                    assert_eq!(set, "Simple");
                } else if sensitivity.is_some() {
                    assert_eq!(set, "CaseInsensitive");
                }
            }
        }
    }
}
