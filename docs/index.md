# smartformat-rs documentation

Four kinds of document, each serving one need. Pick the one that matches why
you came.

- **I want to learn.** The [tutorial](#tutorial) is a lesson: follow it and you
  end up with something that runs.
- **I have a task.** The [how-to guides](#how-to-guides) are recipes for a goal
  you already have.
- **I need to look something up.** The [reference](#reference) is the tables:
  every formatter, specifier, setting and culture.
- **I want to understand why.** The [explanation](#explanation) covers the
  reasoning: what the port is for, what it costs, how it is built.

The API itself is the rustdoc: `cargo doc -p smartformat --all-features --open`.
[DESIGN.md](../DESIGN.md) is the decision record and the ledger of every known
divergence from SmartFormat.NET.

## Tutorial

- [Get started with smartformat](tutorials/getting-started.md): build one
  message from a map, a struct, a plural, a list and a German culture.

## How-to guides

- [Run your existing .NET SmartFormat templates from Rust](how-to/run-dotnet-templates.md):
  map your .NET settings, register what .NET registers by hand, feed values
  without reflection, and validate the corpus.
- [Serve translated text](how-to/localize-text.md): wire up a
  `LocalizationProvider`, choose the culture, and decide what a missing key does.
- [Choose what happens when a template or a value is wrong](how-to/choose-error-behavior.md):
  the four error actions with the output each one produces.
- [Test your templates](how-to/test-your-templates.md): parse checks, snapshots
  with a pinned culture and clock, and goldens generated from real .NET.
- [Add a culture the crate does not ship](how-to/add-a-culture.md): extend the
  generated culture table and regenerate the goldens with it.
- [Write your own formatter or source](how-to/extend-with-your-own.md): the two
  traits, the toolkit each one receives, and where in the registry it lands.

## Reference

- [Template syntax](reference/template-syntax.md): the complete grammar,
  selectors, alignment, nesting, escaping and every parse-error message.
- [Formatters](reference/formatters.md): the registry with .NET's ranks, the
  selection rules, and a section each for `list`, `plural`, `cond`, `ismatch`,
  `isnull`, `choose`, `substr`, `time`, `L`, `t` and the default formatter.
- [Format specifiers](reference/format-specifiers.md): the standard numeric,
  date/time and `TimeSpan` specifiers, with a rendered example for each.
- [Settings and features](reference/settings-and-features.md): every
  `SmartSettings` and `ParserSettings` field, and the cargo feature table.
- [Cultures](reference/cultures.md): the 35 shipped cultures, the lookup and
  validation rules, and the data behind them.

## Explanation

- [Why byte-identical output is the goal](explanation/byte-compatibility.md):
  what refusing to approximate rules out, what it costs, and where UTF-8 meets
  UTF-16.
- [How compatibility is verified](explanation/how-compatibility-is-verified.md):
  the golden-file method, what the harness pins, and what it cannot prove.
- [How a render happens](explanation/architecture.md): parser, tree, engine,
  sources, formatters, and the two ordered registries between them.
- [The performance model, and its evidence](explanation/performance.md): the
  parse-once shape, the allocation-free paths, the benchmark figures and their
  limits.
- [How the port compares to SmartFormat.NET](explanation/dotnet-comparison.md):
  the same templates measured on both runtimes, and what makes that comparison
  imperfect.
