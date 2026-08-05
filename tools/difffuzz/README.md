# difffuzz

A differential fuzzer for this port. It invents SmartFormat templates, renders
each one with the real SmartFormat.NET and with the port, and diffs the two byte
for byte.

The golden corpus in `goldens/m1.json` pins 2,764 cases somebody thought of.
This tool is for the ones nobody thought of.

## What it does

**Generates.** Uniformly random text is useless here — almost none of it parses,
so a whole campaign would exercise one error path. The generator builds a
template out of weighted constructs instead: literal text with escapes (`\{`,
`\}`, `\n`, `\uXXXX`, and the malformed shapes that live next to them),
placeholders with named, positional, nested, nullable and nameless selectors,
alignment in both directions, formatter names with and without options, nested
formats, and the split-based formatters — `list`, `plural`, `cond`, `choose`,
`isnull`, `ismatch`, `substr`, `time`, `t`, `L` — with realistic part counts and
with deliberately wrong ones. Alongside it builds an argument tree the selectors
mostly resolve against, and picks the selector to suit the formatter, so about
three cases in five reach the rendering path rather than the reporting one.

The weights lean towards what the golden corpus is thin on: placeholders nested
inside split parts, alignment combined with formatter options, escapes inside
options, cultures other than the invariant one, and the three `ErrorAction`s
that recover instead of throwing. That last one matters twice over — a
recovering action turns an exception, of which .NET tells us only the type name,
into text that can actually be diffed.

**Compares.** Cases go to `tools/goldens` in batches, and the answers come back
matched up by id. The same cases are rendered in Rust through
`src/rustside.rs`, which mirrors `crates/smartformat/tests/goldens.rs` — the
same argument mapping, the same settings keys, the same localization, variables
and template fixtures. Each disagreement is sorted into one of four classes:

| class              | what it means |
| ------------------ | ------------- |
| `new`              | Nothing in `DESIGN.md` or the golden skip list explains it. Look at this one. |
| `known-divergence` | It has the shape of a divergence the project already decided about. The rule that caught it, and its reason, are in the report. |
| `order-dependent`  | The two disagreed in a batch and agreed when the case ran alone, so what differs is .NET state left over from an earlier case, not the rendering. |
| `harness-died`     | The harness process did not survive the case. |

**Shrinks.** A two-hundred-character template that disagrees says nothing.
Every `new` disagreement is reduced while it keeps disagreeing: items dropped,
literals shortened, selectors simplified, options dropped, split parts removed,
alignment lowered, arguments pruned, settings removed, culture reset. The
reductions come from the syntax tree rather than the text, so a candidate is
still a well-formed template unless it was meant not to be — and a whole round
of candidates goes into **one** `dotnet run`, because a start-up per candidate
would cost more than the campaign that found the case.

## Running a campaign

```
cargo run --manifest-path tools/difffuzz/Cargo.toml -- --seed 12345 --count 500
```

It is a separate crate with its own detached `[workspace]`, like `fuzz/`, so it
never touches the workspace build, its lockfile or its MSRV. It needs the dotnet
SDK on `PATH` and a `tools/goldens` that implements case-file mode:

```
dotnet run --project tools/goldens -- --cases <path>
```

If that mode is not there yet, the first batch says so and stops rather than
blaming 500 cases for a crash that never happened. Use `--no-dotnet` until it
is: the campaign then generates and renders on the Rust side only, which is
still worth running — it is a crash fuzzer for the port on its own, and 5,000
cases take under a second.

Useful options:

| option | |
| ------ | --- |
| `--seed N` | The campaign seed. Without one it is drawn from the clock and printed. |
| `--count N` | How many cases to generate. Default 200. |
| `--index N` | Run only case *N* of the campaign. |
| `--batch-size N` | Cases per `dotnet run`. Default 100. |
| `--shrink-batch N` | Candidate reductions per shrinking round. Default 120. |
| `--shrink-rounds N` | How many rounds one finding is shrunk for. Default 12. |
| `--report PATH` | Where the report goes. Default `difffuzz-report.json`. |
| `--no-confirm-alone` | Skip the isolation re-run. Faster, and `order-dependent` then hides among `new`. |
| `--timeout SECONDS` | How long one `dotnet run` may take before it is killed. Default 600. |
| `--dotnet PATH`, `--repo PATH` | For an unusual layout. |

The exit code is 1 when the campaign found a new disagreement, 0 otherwise.

## Reproducing a seed

The same seed reproduces the same run, exactly, on any machine. Case *n* is
built from a stream derived from `(seed, n)` rather than drawn in sequence, so a
single case is reproducible on its own:

```
cargo run --manifest-path tools/difffuzz/Cargo.toml -- --seed 12345 --index 137
```

That rebuilds case 137 of that campaign and nothing else. The generator uses a
`xoshiro256**` written out in `src/rng.rs` rather than a dependency, because a
dependency is free to change its stream between versions and then a seed in a
report means nothing.

## Reading a report

The report is one JSON document. The top of it is the summary — cases run,
agreements, disagreements by class — and the command that reproduces the run.
Then one entry per disagreement:

```json
{
  "id": "fz-12345-137",
  "class": "new",
  "template": "{0,-5:N2} and {1:list:{}|, | and }",
  "args": [1.5, ["a", "b"]],
  "culture": "de-DE",
  "settings": { "formatErrorAction": "Ignore" },
  "dotnet": { "result": " 1,50 and a, b" },
  "rust":   { "result": "1,50  and a, b" },
  "minimal": { "template": "{0,-5:N2}", "args": [1.5], "culture": "", "settings": null },
  "case": { "id": "…", "template": "…", "args": …, "culture": "…" }
}
```

`template`, `args`, `culture` and `settings` are the four inputs. `dotnet` and
`rust` are what each side said — `{"result": …}`, `{"error": …}`, or, on the
Rust side, `{"panic": …}`, which is the best thing this tool can find. `minimal`
is where the shrinker got to; read that one, not the template above it. `case`
is the whole thing in the shape `goldens/m1.json` uses, ready to paste into the
golden table once it has been triaged.

A `known-divergence` entry carries two more fields: `rule`, the name of the
rule that caught it, and `reason`, the wording from `DESIGN.md` or the golden
skip list that the rule stands for.

**The classification is triage, not proof.** `DESIGN.md` is the ledger and the
skip list is the pin; `src/classify.rs` only recognises the *shape* of an entry
that is already in one of them, so that a campaign's output is a short list of
things to look at rather than a long list of things already decided. The
`ismatch` rules are the broadest — `$` alone covers most anchored patterns — so
a suppressed `ismatch` finding deserves more suspicion than any other.

## What the tests cover

`cargo test --manifest-path tools/difffuzz/Cargo.toml`.

`tests/goldens_replay.rs` runs all 2,833 golden cases back through
`src/rustside.rs` and compares them with the same rules a campaign uses. It is
what catches the mirror drifting from `crates/smartformat/tests/goldens.rs`, and
it holds the classifier to recognising *every* divergence the corpus records —
without an id to look it up by.

`tests/driver.rs` drives the whole pipeline against `src/bin/fake-harness`, a
stand-in that speaks the same case-file contract and can be told to die, to
hang, or to answer half a batch. Those are the failures the real harness only
has by accident, and they are the ones worth being sure about: a stack overflow
takes the CLR with it, so the culprit has to be found by halving the batch
rather than by asking one case at a time.
