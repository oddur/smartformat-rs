# Get started with smartformat

In this tutorial you build one shipping notification and grow it step by step:
first a template over a map of values, then over your own struct, then a
sentence that reads correctly for 0, 1 or 3 packages, then a list of package
names, and finally the same message in German with German numbers.

Every step is a complete `src/main.rs`. Replace the file each time, run it, and
check the output against what the step says you will see.

You need Rust 1.75 or newer.

## Step 1: Start a project

```console
$ cargo new smartformat-tour
$ cd smartformat-tour
```

smartformat is not published on crates.io, so depend on the git repository. Add
this to `Cargo.toml`:

```toml
[dependencies]
smartformat = { git = "https://github.com/oddur/smartformat-rs" }
```

If you already have the repository checked out beside your project, point at
the checkout instead:

```toml
[dependencies]
smartformat = { path = "../smartformat-rs/crates/smartformat" }
```

Fetch and build it:

```console
$ cargo build
```

## Step 2: Render your first template

Put this in `src/main.rs`:

```rust
use std::collections::BTreeMap;

use smartformat::{SmartFormatter, Value};

fn main() {
    let mut order = BTreeMap::new();
    order.insert("Customer".to_string(), Value::from("Alice"));
    order.insert("Total".to_string(), Value::from(1234.5));

    let smart = SmartFormatter::default();

    let message = smart
        .format("{Customer}, your order comes to {Total:N2}.", &Value::Map(order))
        .unwrap();

    println!("{message}");
    assert_eq!(message, "Alice, your order comes to 1,234.50.");
}
```

Run it:

```console
$ cargo run
Alice, your order comes to 1,234.50.
```

`{Customer}` looked up the map key of that name. `{Total:N2}` applied the .NET
number specifier `N2`: group separators and two decimal places.

## Step 3: Feed it your own type

Building a `BTreeMap` by hand gets old. Derive `ToSmartValue` on a struct and
the same template reads its fields. Replace `src/main.rs` with:

```rust
use smartformat::value::ToSmartValue as _;
use smartformat::{SmartFormatter, ToSmartValue};

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Order {
    Customer: String,
    Total: f64,
    Packages: i64,
}

fn main() {
    let order = Order {
        Customer: "Alice".to_string(),
        Total: 1234.5,
        Packages: 3,
    };

    let smart = SmartFormatter::default();

    let message = smart
        .format("{Customer}, your order comes to {Total:N2}.", &order.to_smart_value())
        .unwrap();

    println!("{message}");
    assert_eq!(message, "Alice, your order comes to 1,234.50.");
}
```

```console
$ cargo run
Alice, your order comes to 1,234.50.
```

Same output, new source of data. The derive uses field names verbatim, so
`Customer` in the struct is `{Customer}` in the template. That is why the fields
are `PascalCase` and why the `#[allow(non_snake_case)]` is there: SmartFormat
templates spell .NET property names.

## Step 4: Make one template handle 0, 1 and many

`{Packages:plural:package is|packages are}` picks a word by the number. Replace
`src/main.rs` with:

```rust
use smartformat::value::ToSmartValue as _;
use smartformat::{SmartFormatter, ToSmartValue};

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Order {
    Customer: String,
    Total: f64,
    Packages: i64,
}

fn main() {
    let smart = SmartFormatter::default();
    let template =
        "{Customer}, {Packages} {Packages:plural:package is|packages are} on the way. \
         Total: {Total:N2}.";

    for packages in [0, 1, 3] {
        let order = Order {
            Customer: "Alice".to_string(),
            Total: 1234.5,
            Packages: packages,
        };

        let message = smart.format(template, &order.to_smart_value()).unwrap();
        println!("{message}");
    }

    let one = Order { Customer: "Alice".to_string(), Total: 1234.5, Packages: 1 };
    assert_eq!(
        smart.format(template, &one.to_smart_value()).unwrap(),
        "Alice, 1 package is on the way. Total: 1,234.50.",
    );
}
```

```console
$ cargo run
Alice, 0 packages are on the way. Total: 1,234.50.
Alice, 1 package is on the way. Total: 1,234.50.
Alice, 3 packages are on the way. Total: 1,234.50.
```

One template, three grammatical sentences. English takes the first word for 1
and the second for everything else.

## Step 5: List the packages

Now change `Packages` from a count to the names themselves. Replace
`src/main.rs` with:

```rust
use smartformat::value::ToSmartValue as _;
use smartformat::{SmartFormatter, ToSmartValue};

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Order {
    Customer: String,
    Total: f64,
    Packages: Vec<String>,
}

fn main() {
    let smart = SmartFormatter::default();
    let template =
        "{Customer}, your {Packages:list:{}|, |, and } {Packages:plural:is|are} on the way. \
         Total: {Total:N2}.";

    let order = Order {
        Customer: "Alice".to_string(),
        Total: 1234.5,
        Packages: vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "monitor".to_string(),
        ],
    };

    let message = smart.format(template, &order.to_smart_value()).unwrap();
    println!("{message}");
    assert_eq!(
        message,
        "Alice, your keyboard, mouse, and monitor are on the way. Total: 1,234.50.",
    );

    let single = Order {
        Customer: "Alice".to_string(),
        Total: 19.99,
        Packages: vec!["keyboard".to_string()],
    };

    let message = smart.format(template, &single.to_smart_value()).unwrap();
    println!("{message}");
    assert_eq!(message, "Alice, your keyboard is on the way. Total: 19.99.");
}
```

```console
$ cargo run
Alice, your keyboard, mouse, and monitor are on the way. Total: 1,234.50.
Alice, your keyboard is on the way. Total: 19.99.
```

`{Packages:list:{}|, |, and }` renders each item with `{}`, puts `, ` between
them and `, and ` before the last. The plural placeholder did not change at all:
given a list, it counts the items.

## Step 6: Render it in German

`format` uses the invariant culture. `format_with_culture_name` takes a culture
name and formats the numbers the way that culture does. Replace `src/main.rs`
with:

```rust
use smartformat::value::ToSmartValue as _;
use smartformat::{SmartFormatter, ToSmartValue};

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Order {
    Customer: String,
    Total: f64,
    Packages: Vec<String>,
}

fn main() {
    let smart = SmartFormatter::default();

    let english =
        "{Customer}, your {Packages:list:{}|, |, and } {Packages:plural:is|are} on the way. \
         Total: {Total:N2}.";
    let german =
        "{Customer}, {Packages:list:{}|, |, und } {Packages:plural:ist|sind} unterwegs. \
         Summe: {Total:N2}.";

    let order = Order {
        Customer: "Alice".to_string(),
        Total: 1234.5,
        Packages: vec![
            "Tastatur".to_string(),
            "Maus".to_string(),
            "Monitor".to_string(),
        ],
    };
    let args = order.to_smart_value();

    let en = smart.format_with_culture_name(english, &args, "en-US").unwrap();
    let de = smart.format_with_culture_name(german, &args, "de-DE").unwrap();

    println!("{en}");
    println!("{de}");

    assert_eq!(
        en,
        "Alice, your Tastatur, Maus, and Monitor are on the way. Total: 1,234.50.",
    );
    assert_eq!(
        de,
        "Alice, Tastatur, Maus, und Monitor sind unterwegs. Summe: 1.234,50.",
    );
}
```

```console
$ cargo run
Alice, your Tastatur, Maus, and Monitor are on the way. Total: 1,234.50.
Alice, Tastatur, Maus, und Monitor sind unterwegs. Summe: 1.234,50.
```

Look at the total: `1,234.50` in English, `1.234,50` in German. The culture data
is read out of .NET itself, so `de-DE` here groups and points exactly as `de-DE`
does there. A name with no data behind it is an error rather than a guess, so a
typo tells you:

```rust
use smartformat::{SmartFormatter, Value};

fn main() {
    let smart = SmartFormatter::default();
    let args = Value::List(vec![Value::Float(1234.5)]);

    assert!(smart.format_with_culture_name("{0:N2}", &args, "de-XX").is_err());
}
```

## What you built

A message that names your data, agrees with its own count, lists items with a
final `and`, and localizes its numbers, from one template per language and one
Rust struct. You used:

- `SmartFormatter::default()` and `format`, the whole entry point.
- `#[derive(ToSmartValue)]`, which makes a struct addressable by its own field
  names.
- `{Total:N2}`, a .NET format specifier.
- `{Packages:plural:is|are}` and `{Packages:list:{}|, |, and }`, two of the ten
  formatters.
- `format_with_culture_name`, which swaps the locale under a fixed template.

## Where to go next

- Bringing templates over from a .NET codebase:
  [Run .NET SmartFormat templates from Rust](../how-to/run-dotnet-templates.md).
- Every formatter, its options and its output:
  [Formatters](../reference/formatters.md).
- Why byte-identical output with SmartFormat.NET is the goal, and what it costs:
  [Byte compatibility](../explanation/byte-compatibility.md).
