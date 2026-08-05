# Template syntax

The complete grammar smartformat-rs parses. It is SmartFormat.NET's grammar; a
template that parses there parses here, with the same tree and the same error
messages.

Source of truth: `crates/smartformat/src/parsing/`.

## Placeholder anatomy

A template is literal text interleaved with placeholders. A placeholder is
everything between `{` and `}`:

```text
{selectors,alignment:formatterName(formatterOptions):format}
```

| Part | Delimiter | Optional | Notes |
| --- | --- | --- | --- |
| selectors | none (first), `.` `?.` `[` `]` between | yes | `{}` has none |
| alignment | `,` before it | yes | signed integer; parsed as a selector, skipped when values are resolved |
| formatter name | `:` before it | yes | not parsed in `string_format_compatibility` mode |
| formatter options | `(` … `)` after the name | yes | only with a formatter name |
| format | `:` after the name or options | yes | everything up to the matching `}` |

Every part after the selectors is optional, and the whole placeholder can be
just `{}`. Examples of the shapes the parser distinguishes:

| Template | Selectors | Alignment | Formatter | Options | Format |
| --- | --- | --- | --- | --- | --- |
| `{0}` | `0` | 0 | none | none | none |
| `{0:N2}` | `0` | 0 | none | none | `N2` |
| `{Items.Length,-10:choose(1\|2):one\|two}` | `Items`, `Length`, `-10` | -10 | `choose` | `1\|2` | `one\|two` |
| `{0:d:N2}` | `0` | 0 | `d` | none | `N2` |
| `{:t(firstLast)}` | none | 0 | `t` | `firstLast` | empty |
| `{}` | none | 0 | none | none | none |

A placeholder with no formatter name and a format goes to auto-detection; see
[formatters.md](formatters.md).

### Formatter name grammar

The text after the first `:` is read as a formatter name only if it matches
`name` or `name(options)` followed by `:` or `}`. Anything else leaves the
placeholder without a formatter name, and the text stays part of the format.

| Written | Formatter name | Format |
| --- | --- | --- |
| `{0:list:{}\|, }` | `list` | `{}\|, }` up to the matching brace |
| `{0:list()}` | `list` | empty |
| `{0:N2}` | none | `N2` |
| `{0::N2}` | none (empty name) | `:N2` |
| `{0:list(x}` | none (unclosed `(`) | `list(x` |
| `{0:list(x)y}` | none (`)` not followed by `:` or `}`) | `list(x)y` |

Formatter options end at an unescaped `:`, `(`, `)`, `{` or `}`.

## Selectors

Selectors are resolved left to right, each against the value the one before it
produced. The first one is resolved against the current scope, with a fallback
described under [Nesting and scope](#nesting-and-scope).

### Operators

Contiguous operator characters form one operator. The operator characters are
`.`, `?`, `,`, `[` and `]`, plus any added through
[`ParserSettings::custom_operator_chars`](settings-and-features.md#parsersettings).

| Operator | Written | Meaning |
| --- | --- | --- |
| (none) | `{Name}` | first selector of the placeholder |
| `.` | `{Person.Name}` | member of the previous value |
| `?.` | `{Person?.Name}` | member, but the whole chain short-circuits to null if any value in it is null |
| `[` | `{Items[0]}` | list index; the index is a selector of its own carrying the `[` operator |
| `]` | `{Items[0]}` | closes the index; contributes an empty trailing selector, which evaluation skips |
| `].` | `{Items[0].Name}` | closes the index and takes a member |
| `?[` | `{Items?[0]}` | index, null-conditional |
| `,` | `{Name,10}` | alignment; the selector after it is the width, never resolved as a value |

The nullable operator is read from the whole selector chain, not just the
selectors before the null: `{City.Length?.Nope}` renders empty when `City` is
null. That is SmartFormat.NET 3.6.1 behaviour, pinned by the goldens.

An empty selector is skipped rather than resolved, so `{0..Length}` behaves as
`{0.Length}` and `{}` resolves to the current scope value.

### Selector characters

| Filter | Accepts | Default |
| --- | --- | --- |
| `SelectorFilter::Alphanumeric` | `0-9`, `a-z`, `A-Z`, `_`, `-`, plus `custom_selector_chars` | yes |
| `SelectorFilter::VisualUnicodeChars` | every Unicode character except 68 non-visual ones, the operator characters, the delimiters `: { } ( )` and `\` | no |

A character outside the filter is the parse error `Invalid character in the
selector`.

### Positional and named selectors

| Form | Resolved by | Rule |
| --- | --- | --- |
| `{0}`, `{1}` | `DefaultSource` | decimal index into the argument list; must be the first selector, carry no operator and be in range |
| `{Name}` | `MapSource`, variable sources, … | matched against map keys and variable-group names, honouring [`SmartSettings::case_sensitive`](settings-and-features.md#smartsettings) |
| `{Length}` on a string | `StringSource` | string members .NET exposes |
| `{Index}` inside a `list` format | `ListSource` | index of the item being rendered; `-1` outside any list |

A positional selector is only positional in the first slot: `{Person.0}` asks
sources for a member named `0`.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::from("Joe"), Value::Int(42)]);
assert_eq!(smart.format("{0} is {1}", &args).unwrap(), "Joe is 42");
```

## Alignment

The alignment is a selector introduced by `,`. Its text is trimmed and parsed
as an `i32`; text that is not an integer leaves the alignment at 0.

| Sign | Effect |
| --- | --- |
| positive | pad on the left (right-align) |
| negative | pad on the right (left-align) |
| 0 or unparseable | no padding |

Width is counted in UTF-16 code units, as .NET counts string length. The pad
character is [`SmartSettings::alignment_fill_character`](settings-and-features.md#smartsettings),
a space by default. A nested placeholder inherits the alignment of the
placeholder it sits in.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let args = Value::List(vec![Value::from("x")]);
assert_eq!(smart.format("{0,5}|", &args).unwrap(), "    x|");
assert_eq!(smart.format("{0,-5}|", &args).unwrap(), "x    |");
// Not an integer: no alignment, no error.
assert_eq!(smart.format("{0,y}|", &args).unwrap(), "x|");
```

## Nesting and scope

A placeholder's format may contain further placeholders. The value each nested
placeholder sees depends on the formatter that renders the format:

| Formatter | Value pushed as the scope of the nested format |
| --- | --- |
| default (`d`) | the placeholder's own value |
| `list` | the current item; spacers see the value the call was made with |
| `plural`, `cond`, `isnull`, `choose`, `t`, `L` | the placeholder's own value |
| `ismatch` | the placeholder's own value, plus the capture group list under `m` |
| `substr` | the substring |

Inside a nested format, `{}` is that pushed value.

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let items = Value::List(vec![Value::List(vec![Value::from("a"), Value::from("b")])]);
assert_eq!(smart.format("{0:list:[{}]|, }", &items).unwrap(), "[a], [b]");
```

Selector resolution falls back outward: when the *first* selector of a nested
placeholder resolves against nothing in the current scope, the enclosing
scopes are tried, innermost first. Only the first selector falls back; a later
one that fails is an error.

```rust
use smartformat::{SmartFormatter, Value};

let mut root = std::collections::BTreeMap::new();
root.insert("Sep".to_owned(), Value::from(" & "));
root.insert(
    "Tags".to_owned(),
    Value::List(vec![Value::from("a"), Value::from("b")]),
);

let smart = SmartFormatter::default();
// `Sep` is not on an item, so it resolves against the enclosing scope.
assert_eq!(
    smart.format("{Tags:list:{}{Sep}|}", &Value::Map(root)).unwrap(),
    "a & b & "
);
```

Nesting depth is recorded on each placeholder (`Placeholder::nested_depth`,
starting at 1) but the engine tracks scopes rather than depth.

## Escaping

The escape character is `\`. It is recognized in literal text and in formatter
options; nothing else escapes.

| Sequence | Produces | Where |
| --- | --- | --- |
| `\\` | `\` | literals and options |
| `\{` | `{` | literals and options |
| `\}` | `}` | literals and options |
| `\:` | `:` | literals and options |
| `\(` | `(` | options only |
| `\)` | `)` | options only |
| `\0` | U+0000 | literals and options, when `convert_character_string_literals` is on |
| `\a` | U+0007 | same |
| `\b` | U+0008 | same |
| `\f` | U+000C | same |
| `\n` | U+000A | same |
| `\r` | U+000D | same |
| `\t` | U+0009 | same |
| `\v` | U+000B | same |
| `\uXXXX` | the UTF-16 code unit `XXXX` | literals, when `convert_character_string_literals` is on; options, always |

Rules for `\uXXXX`:

- Four characters are taken, or fewer at the end of the input, and parsed as
  .NET's `int.TryParse(NumberStyles.HexNumber)` parses them: leading and
  trailing spaces (and U+0009–U+000D) are skipped, and neither a sign nor a
  `0x` prefix is allowed.
- A high surrogate followed by an escaped low surrogate is joined into one
  character. A lone surrogate becomes U+FFFD, because a Rust `String` cannot
  hold half a pair.
- The parser resumes reading *inside* the sequence, so a `{`, `}` or `\` among
  the four characters is read again as literal text. This is what produces the
  out-of-range split errors described in `Format::split`.

With `convert_character_string_literals` off, only `\\` still resolves; every
other sequence stays as written.

Escape failures are **not** parse errors. The sequence is recorded on the item
and raised as `Error::Escape` when the text is written or the options are read,
so a format that never becomes text — `{0:0.00}`, whose format reaches the
value as a specifier — never rejects its sequences. The one exception is an
escape character at the very end of the input, which fails the parse whatever
the error action is.

| Message | Raised when |
| --- | --- |
| `Unrecognized escape sequence "\q" in literal.` | the character after `\` starts no sequence |
| `Unrecognized escape sequence in literal: "\uZZZZ"` | the four characters after `\u` are not hex |
| `Unrecognized escape sequence at the end of the literal` | the template ends with `\`; fails the parse |

```rust
use smartformat::{SmartFormatter, Value};

let smart = SmartFormatter::default();
let none = Value::Null;
assert_eq!(smart.format(r"\{literal\}", &none).unwrap(), "{literal}");
assert_eq!(smart.format(r"a\tb", &none).unwrap(), "a\tb");
assert_eq!(smart.format(r"\u0041\u0042", &none).unwrap(), "AB");
assert!(smart.format(r"\q", &none).is_err());
```

### string.Format compatibility

With [`string_format_compatibility`](settings-and-features.md#parsersettings)
on, braces are doubled instead of backslash-escaped, and formatter names are
not parsed at all: every placeholder goes to `DefaultFormatter`.

| Written | Renders |
| --- | --- |
| `{{` | `{` |
| `}}` | `}` |
| `{0:N2}` | the value with the `N2` specifier |
| `{0:list:{}\|, }` | the `list` name is not parsed; the text is a specifier |

```rust
use smartformat::{SmartFormatter, SmartSettings, Value};

let mut settings = SmartSettings::default();
settings.string_format_compatibility = true;
let smart = SmartFormatter::new(settings);

let args = Value::List(vec![Value::Float(1234.5)]);
assert_eq!(smart.format("{{0}} = {0:N2}", &args).unwrap(), "{0} = 1,234.50");
```

Backslash escaping still runs in this mode when
`convert_character_string_literals` is on, which is the default.

## Parse errors

Syntax errors are collected during the single parse pass and then handled per
[`ParserSettings::error_action`](settings-and-features.md#parsersettings).
Positions are counted in UTF-16 code units.

| Message | Cause |
| --- | --- |
| `Format string is missing a closing brace` | a placeholder is still open at the end of the input |
| `Format string has too many closing braces` | a `}` with no placeholder to close |
| `'0xNN': There are illegal trailing operators in the selector` | an operator with no selector after it, as in `{0.}` |
| `'0xNN': Invalid character in the selector` | a character the selector filter rejects, as in `{0.$}` |

`0xNN` is the offending character's code point in upper-case hex.

With `ErrorAction::Error`, `Error::Parse` carries one `ParseError` per issue and
a combined message in .NET's own layout:

```text
The format string has 1 issue:
'0x2E': There are illegal trailing operators in the selector
In: "{0.}"
At:  --^ 
```

The other error actions recover and return a tree: `MaintainTokens` turns each
erroneous placeholder back into literal text, `Ignore` drops it, and
`OutputErrorInResult` replaces the whole template with the message above.

## Related

- [formatters.md](formatters.md): what each formatter does with the parts.
- [format-specifiers.md](format-specifiers.md): the specifiers a format can be.
- [settings-and-features.md](settings-and-features.md): every parser setting.
- `DESIGN.md` in the repository root: the divergence ledger, including the
  parser quirks reproduced on purpose.
