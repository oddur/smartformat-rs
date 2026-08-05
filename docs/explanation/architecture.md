# How a render happens

A render is a walk over a parsed tree. Template text goes into the parser and comes out as a
`Format`; the engine walks the format's items in order; a literal is written as it stands;
a placeholder has its selector chain resolved by the *sources* and its value written by a
*formatter*. Everything else in the crate is a detail hanging off that sentence.

```text
template  ->  Parser  ->  Format  ->  engine walk  ->  output
                            |            |
                            |            +-- selectors -> SourceRegistry -> Value
                            |            +-- value     -> FormatterRegistry -> text
                            +-- items: LiteralText | Placeholder
```

The shape is SmartFormat.NET's, deliberately. `parsing` is a port of
`Core/Parsing/`, `formatter` is `SmartFormatter.cs` plus `Evaluator.cs` plus
`FormattingInfo.cs`, `sources` is `ISource` and its implementations, and `extensions` is
`IFormatter` and its implementations. Keeping the seams where .NET puts them is what makes a
divergence findable: when the output differs, the two codebases can be read side by side.

## The parse tree

`Parser::parse` produces a `Format`, which is a vector of items plus the raw text and the
byte range it spans. An item is either a `LiteralText` (text outside braces, with escape
sequences resolved) or a `Placeholder` (a selector chain, an alignment, a formatter name,
formatter options, and optionally a nested `Format`).

Every node keeps the byte range it was parsed from and the raw text of that range. That
costs memory, and it buys two things the port needs. Error messages quote the template and
point a caret at an offset, which .NET does from the same information. And a format can be
*cut* into pieces along a separator: `{0:choose(a|b):yes|no|other}` needs its child format
split into three, and each piece has to know where it came from so that it renders, and
reports errors, exactly as the whole would have.

A parse tree is meant to be read, not edited. `items`, `raw`, `start` and `end` are
accessors rather than public fields, and a changed tree is built with `Format::new` out of
the parts of an existing one. The reason is caching: a format remembers the pieces it was cut
into, and a caller who mutated an item between two renders would be rendering the format it
used to be. [The performance model](performance.md) covers what that store buys.

## The walk

`SmartFormatter::format_parsed` builds an `Engine` for the call and walks the top-level
format. The engine holds what .NET's `FormatDetails` holds: the arguments, the culture, the
template text errors quote, plus two pieces of state .NET keeps in statics (the list
formatter's collection index and the culture a formatter switched to mid-call).

For each item:

- **A literal** is written, padded to the alignment of the format it sits in.
- **A placeholder** has its selectors resolved one at a time. The first selector is looked
  up against the current value and, failing that, against each enclosing scope from the
  inside out; later selectors are looked up against the value the previous one produced. A
  selector nothing answers is a formatting error.
- **The resolved value** goes to a formatter, which writes text through a `FormattingInfo`.

A formatter that renders a child format re-enters the walk with the child value pushed onto
the scope chain, which is how nesting works: `{Person:{First} {Last}}` is one placeholder
whose format is rendered against `Person`.

The arguments themselves follow .NET's convention. A `Value::List` passed to `format` is the
positional argument set, and its first element is the initial current value; any other value
is a single argument that is also the current value.

## The Value model

Rust has no reflection, and this is the design decision the rest of the port bends around.
.NET's SmartFormat resolves `{Customer.Address.City}` by reflecting over whatever object it
was handed. Nothing in Rust can do that, so the port inverts the relationship: the caller
converts data into a `Value` tree, and selectors resolve against that.

`Value` is a small enum: `Null`, `Bool`, `Int`, `UInt`, `Float`, `String`, `List`, `Map`, and
behind the `time` feature `DateTime` and `TimeSpan`. `#[derive(ToSmartValue)]` is what
replaces reflection: it walks a struct's named fields at compile time and emits the
`Value::Map` construction. Field names go in verbatim, so a struct's field names are what
templates spell.

```rust
use smartformat::value::ToSmartValue as _;
use smartformat::{SmartFormatter, ToSmartValue, Value};

#[derive(ToSmartValue)]
#[allow(non_snake_case)]
struct Order {
    Customer: String,
    Items: Vec<String>,
}

let order = Order {
    Customer: "Ada".to_owned(),
    Items: vec!["sword".to_owned(), "shield".to_owned()],
};

// The derive produces exactly what a hand-built map would.
assert_eq!(
    order.to_smart_value(),
    Value::Map(
        [
            ("Customer".to_owned(), Value::from("Ada")),
            (
                "Items".to_owned(),
                Value::List(vec![Value::from("sword"), Value::from("shield")]),
            ),
        ]
        .into_iter()
        .collect()
    )
);

let smart = SmartFormatter::default();
assert_eq!(
    smart.format("{Customer}: {Items:list:{}|, |, and }", &order.to_smart_value()).unwrap(),
    "Ada: sword, and shield"
);
```

The trade is explicit. Conversion costs a pass over the data and produces a snapshot rather
than a live view of an object graph, and a struct whose fields are named in Rust style needs
either `#[allow(non_snake_case)]` or templates that spell the Rust names. In exchange,
selector resolution is a match on a small enum instead of a runtime type walk, the failure
modes are visible at the type level, and the derive rejects what it cannot represent:
tuple structs, unit structs and enums fail to compile rather than producing a map with no
useful keys.

What is *not* lost is the method-like selectors, because those were never reflection in .NET
either. `{Name.Length}`, `{Name.ToUpper}` and friends come from `StringSource`, a source
extension, and the port has the same one.

One consequence worth knowing: `Value::Map` is a `BTreeMap`, so it has sorted keys and no
insertion order. Under a case-insensitive setting .NET resolves an ambiguous key by insertion
order, which a `BTreeMap` cannot reproduce; that is a documented divergence rather than an
oversight.

## The two registries

Extensions live in two ordered lists, and in both of them the order is part of the
behavior.

**`SourceRegistry`** answers "given this value and this selector, what is the next value?".
Each source returns `None` for "not my selector", and the first source that answers wins.
The default order is .NET's: `StringSource` (rank 3000), `ListSource` (4000), `MapSource`
(5000), `DefaultSource` (12000). Registering variables puts `GlobalVariablesSource` (1000) or
`PersistentVariablesSource` (2000) ahead of all of them.

Those ranks are not decoration. `ListSource` answering `{Index}` at 4000, ahead of
`MapSource` at 5000, is why `{0:list:{Index}|, }` over a list of maps that each carry an
`Index` key renders the iteration index and not the key. Moving a source changes what
existing templates mean.

**`FormatterRegistry`** answers "write this value". A placeholder that names a formatter
reaches that one and no other. A placeholder that names none is offered to every
auto-detecting formatter in registry order until one claims it, which makes the order
directly observable:

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();

// `list` is ranked ahead of `plural`, and it claims any `|`-separated format
// on a list: "one" becomes the item format and "many" the separator.
let items = Value::List(vec![Value::List(vec![Value::from("a"), Value::from("b")])]);
assert_eq!(smart.format("{0:one|many}", &items).unwrap(), "amanyb");

// The same template on a number reaches `plural`, because `list` declines it.
let count = Value::List(vec![Value::Int(3)]);
assert_eq!(smart.format("{0:one|many}", &count).unwrap(), "many");
```

The default order is .NET's `CreateDefaultSmartFormat` order, by the ranks
`WellKnownExtensionTypes` assigns: `list` 1000, `plural` 2000, `cond` 3000, `time` 4000,
`ismatch` 6000, `isnull` 7000, `L` 8000, `t` 9000, `choose` 10000, `substr` 11000, `d`
(the default formatter) 12000. Three of them are not registered by default, exactly as .NET
leaves them out, because each is useless until it is given something: `time` a language, `L`
a provider, `t` a template.

Adding an extension has three doors, and picking the wrong one is the classic mistake, in
.NET as much as here:

- `add` places it at .NET's rank for its name, which is what the registration helpers use.
- `insert` places it at a chosen index, which is what a formatter of one's own needs.
- `push` appends it, which for the formatter registry means *after* the default formatter,
  where it never runs. .NET has the same trap: an extension its rank table does not know
  goes last.

## Where the extension points are

Three traits and a handful of constructors and setters cover everything a host can change:

| Extension point | What it is for |
|---|---|
| `Formatter` | A new `{value:name:…}` formatter. Five methods, two of them required. |
| `Source` | A new selector, or a new kind of value to resolve selectors against. Two methods, one required. |
| `LocalizationProvider` | Where translations come from. One method, and no resx anywhere. |
| `TimeFormatter::add_language` | A `TimeTextInfo`: the words one language spells durations with. |
| `PluralLocalizationFormatter::with_custom_rule` | A pluralization rule the ported table does not have. |
| `ParserSettings` | Selector characters, escaping mode, parse-error action. |
| `SmartSettings` | Error actions, case sensitivity, alignment fill, the clock. |

A formatter receives a `FormattingInfo`, which is the whole toolkit: the current value, the
placeholder and its format, the alignment, the culture in force, `write`, the two nested
render calls (`format_as_child` and `format_as_child_of_current`), and constructors for
errors that carry .NET's exact message envelope. Formatters do not touch the output string
directly and do not build error text by hand, so a change to either lands everywhere at once.

The practical version of all of this, with working code, is
[write your own formatter or source](../how-to/extend-with-your-own.md).

## State that .NET keeps in statics

Three pieces of state are process-wide in SmartFormat.NET because its extensions are shared
singletons: the list formatter's collection index, the clock, and the global variables store.
Each of them belongs to something narrower here.

The collection index lives on the format call, so two threads rendering at once cannot
disturb each other and an index left behind by a failed iteration cannot leak into the next
call. .NET leaks it, reproducibly, which is why the golden harness renders an `{Index}`
canary after every case. The clock is `SmartSettings::now`, per formatter. The global
variables store is an `Arc` handle rather than a `static`, so sharing it is a clone and
resetting it is a method on the handle.

The reason those choices are safe is that a `SmartFormatter` carries no mutable per-call
state at all, which makes it shareable:

```rust
fn assert_shareable<T: Send + Sync>() {}
assert_shareable::<smartformat::SmartFormatter>();
assert_shareable::<smartformat::parsing::Format>();
```

One formatter behind an `Arc`, a `Format` per template parsed once, and any number of
threads rendering: that is the shape the API is built for, and
[the performance model](performance.md) explains what it is worth.
