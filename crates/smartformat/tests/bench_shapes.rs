//! Pins that the cross-runtime benchmark measures the same work on both sides.
//!
//! `crates/smartformat/benches/render.rs` and `tools/benchmark` are compared
//! against each other in [the .NET comparison][doc], and that comparison is
//! worth nothing unless each shape's template, values and rendered bytes are
//! identical in both. The bytes below were printed by the .NET half
//! (`dotnet run --project tools/benchmark -c Release -- --verify`) and are
//! asserted by its `RenderBenchmarks.Setup`; this file asserts the same table
//! for the Rust half.
//!
//! [doc]: ../../../docs/explanation/dotnet-comparison.md
//!
//! A benchmark is not a test, so nothing else would catch a shape drifting.
//! Change a benchmark template and this fails until the twin changes with it.

use std::collections::BTreeMap;

use smartformat::{SmartFormatter, Value};

/// The values both halves render: a name, a count, a gender flag and a
/// three-item list. The .NET half builds the same thing as a
/// `Dictionary<string, object>` holding a `long` and an `object[]`.
fn person() -> Value {
    Value::Map(BTreeMap::from([
        ("Name".to_owned(), Value::from("Alice")),
        ("Count".to_owned(), Value::Int(3)),
        ("Gender".to_owned(), Value::from("f")),
        (
            "Items".to_owned(),
            Value::List(vec![
                Value::from("sword"),
                Value::from("shield"),
                Value::from("potion"),
            ]),
        ),
    ]))
}

/// Every shape whose template appears in both benchmark suites, with the bytes
/// it must render to. `render_number_spec_de` is the one row under a culture
/// other than the invariant one, because it is the row about a culture.
const SHAPES: &[(&str, &str, &str)] = &[
    (
        "render_selectors",
        "Hello {Name}, you have {Count} items",
        "Hello Alice, you have 3 items",
    ),
    ("render_number_spec", "{0:N2}", "1,234.50"),
    ("render_number_spec_de", "{0:N2}", "1.234,50"),
    (
        "render_plural",
        "{Count:plural:one item|{} items}",
        "3 items",
    ),
    ("render_choose", "{Gender:choose(m|f):his|her}", "her"),
    (
        "render_list",
        "{Items:list:{}|, |, and }",
        "sword, shield, and potion",
    ),
    (
        "render_nested",
        "{Name} has {Count:plural:one item|{} items} in {Gender:choose(m|f):his|her} cart",
        "Alice has 3 items in her cart",
    ),
];

#[test]
fn every_benchmark_shape_renders_what_dotnet_renders() {
    let smart = SmartFormatter::default();
    let args = person();
    let one_float = Value::List(vec![Value::Float(1234.5)]);

    for &(name, template, expected) in SHAPES {
        let format = smart
            .parse(template)
            .unwrap_or_else(|e| panic!("{name}: template does not parse: {e}"));
        let values = if name.starts_with("render_number_spec") {
            &one_float
        } else {
            &args
        };
        let actual = if name == "render_number_spec_de" {
            smart.format_parsed_with_culture_name(&format, values, "de-DE")
        } else {
            smart.format_parsed(&format, values)
        }
        .unwrap_or_else(|e| panic!("{name}: render failed: {e}"));

        assert_eq!(actual, expected, "{name}");
    }
}

/// The one-shot row parses and renders in the same call on both sides, because
/// neither library caches a parsed format.
#[test]
fn the_oneshot_shape_renders_what_dotnet_renders() {
    let smart = SmartFormatter::default();
    let rendered = smart
        .format("Hello {Name}, you have {Count} items", &person())
        .expect("one-shot render");
    assert_eq!(rendered, "Hello Alice, you have 3 items");
}
