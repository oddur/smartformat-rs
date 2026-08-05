//! Render-path benchmarks: parse once, format many, which is the intended
//! production shape. `format_oneshot` measures parse + render together for
//! callers that do not cache.

use std::collections::BTreeMap;

use criterion::{criterion_group, criterion_main, Criterion};
use smartformat::parsing::Format;
use smartformat::sources::variables::{self, PersistentVariablesSource};
use smartformat::{SmartFormatter, Value};
use std::hint::black_box;

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

fn cached(smart: &SmartFormatter, template: &str) -> Format {
    smart.parse(template).expect("benchmark template parses")
}

fn benches(c: &mut Criterion) {
    let smart = SmartFormatter::default();
    let args = person();
    let one_float = Value::List(vec![Value::Float(1234.5)]);

    c.bench_function("parse_simple", |b| {
        b.iter(|| smart.parse(black_box("Hello {Name}, you have {Count} items")))
    });
    c.bench_function("parse_nested", |b| {
        b.iter(|| {
            smart.parse(black_box(
                "{Name} has {Count:plural:one item|{} items} in {Gender:choose(m|f):his|her} cart",
            ))
        })
    });

    let simple = cached(&smart, "Hello {Name}, you have {Count} items");
    c.bench_function("render_selectors", |b| {
        b.iter(|| smart.format_parsed(black_box(&simple), &args))
    });

    let spec = cached(&smart, "{0:N2}");
    c.bench_function("render_number_spec", |b| {
        b.iter(|| smart.format_parsed(black_box(&spec), &one_float))
    });
    c.bench_function("render_number_spec_de", |b| {
        b.iter(|| smart.format_parsed_with_culture_name(black_box(&spec), &one_float, "de-DE"))
    });

    let plural = cached(&smart, "{Count:plural:one item|{} items}");
    c.bench_function("render_plural", |b| {
        b.iter(|| smart.format_parsed(black_box(&plural), &args))
    });

    let choose = cached(&smart, "{Gender:choose(m|f):his|her}");
    c.bench_function("render_choose", |b| {
        b.iter(|| smart.format_parsed(black_box(&choose), &args))
    });

    let list = cached(&smart, "{Items:list:{}|, |, and }");
    c.bench_function("render_list", |b| {
        b.iter(|| smart.format_parsed(black_box(&list), &args))
    });

    // The same template `parse_nested` parses, rendered: three placeholders,
    // two of which reach a formatter that splits the format. The .NET
    // comparison wants a compound shape as well as the single-formatter ones.
    let nested = cached(
        &smart,
        "{Name} has {Count:plural:one item|{} items} in {Gender:choose(m|f):his|her} cart",
    );
    c.bench_function("render_nested", |b| {
        b.iter(|| smart.format_parsed(black_box(&nested), &args))
    });

    // The same three formats with the formatter name left out, so the engine
    // walks the auto-detecting formatters in order and more than one of them
    // splits the format. `list` splits with a limit where the others split
    // without one, so these measure a format that is cut two ways — the shape
    // a single-slot split cache cannot serve.
    let plural_auto = cached(&smart, "{Count:one item|{} items}");
    c.bench_function("render_plural_autodetect", |b| {
        b.iter(|| smart.format_parsed(black_box(&plural_auto), &args))
    });

    let cond_auto = cached(&smart, "{0:yes|no}");
    let one_bool = Value::List(vec![Value::Bool(true)]);
    c.bench_function("render_cond_autodetect", |b| {
        b.iter(|| smart.format_parsed(black_box(&cond_auto), &one_bool))
    });

    let list_auto = cached(&smart, "{Items:{}|, |, and }");
    c.bench_function("render_list_autodetect", |b| {
        b.iter(|| smart.format_parsed(black_box(&list_auto), &args))
    });

    // A registered variables source, which resolves `{group.variable}` without
    // the group being passed as an argument.
    let mut with_variables = SmartFormatter::default();
    with_variables.register_variables(PersistentVariablesSource::from_iter([(
        "app",
        variables::group([
            ("name", Value::from("Acme")),
            ("version", Value::from("1.4.2")),
            ("vendor", Value::from("Globex")),
        ]),
    )]));
    let no_args = Value::List(Vec::new());
    let vars = cached(&with_variables, "{app.name} {app.version}");
    c.bench_function("render_variables", |b| {
        b.iter(|| with_variables.format_parsed(black_box(&vars), &no_args))
    });

    c.bench_function("format_oneshot", |b| {
        b.iter(|| {
            smart.format(
                black_box("Hello {Name}, you have {Count} items"),
                black_box(&args),
            )
        })
    });
}

criterion_group!(render, benches);
criterion_main!(render);
