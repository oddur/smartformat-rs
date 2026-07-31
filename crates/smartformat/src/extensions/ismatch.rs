//! Port of SmartFormat.NET's `IsMatchFormatter`, backed by fancy-regex (milestone M3).
//!
//! Ported from `src/SmartFormat/Extensions/IsMatchFormatter.cs`.
//!
//! `{value:ismatch(regex):matched|not matched}` renders the first of the two
//! `|`-separated parts when the regular expression matches the value and the
//! second when it does not. The matched part may reach the regular
//! expression's capturing groups through the "magic" placeholder `{m[1]}`:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! let smart = SmartFormatter::default();
//!
//! let args = Value::List(vec![Value::from("Some123Content")]);
//! // The options carry SmartFormat's own escaping, so the pattern the engine
//! // sees is `^\D+(\d+)\D+$`: `\(` and `\)` are the escapes for the
//! // parentheses of the capturing group, and `\\d` the one for the backslash
//! // of `\d`.
//! let template = r"{0:ismatch(^\\D+\(\\d+\)\\D+$):digits {m[1]}|no digits}";
//! assert_eq!(smart.format(template, &args).unwrap(), "digits 123");
//! ```
//!
//! # Divergences from .NET
//!
//! The engine is [fancy-regex], not .NET's `System.Text.RegularExpressions`,
//! and the two do not accept or interpret quite the same patterns. The port
//! never silently accepts a pattern it would match differently *when it can
//! tell*: a pattern fancy-regex cannot parse fails the format call. The
//! divergences that survive are the ones where both engines accept a pattern
//! and disagree about it, so they are listed here and in DESIGN.md:
//!
//! * `$` in .NET matches at the end of the input **and** just before a final
//!   `\n`; in fancy-regex it only matches at the end. `^abc$` matches
//!   `"abc\n"` in .NET, not here. (`\z` behaves the same in both; .NET's `\Z`
//!   has no counterpart and fails to parse, which is the loud kind of
//!   divergence.)
//! * .NET numbers named capturing groups *after* all unnamed ones, so
//!   `(a)(?<x>b)(c)` matched against `abc` gives group 2 = `c` and group 3 =
//!   `b`; fancy-regex numbers groups left to right, so `{m[2]}` is `b` and
//!   `{m[3]}` is `c`. Patterns that mix named and unnamed groups therefore
//!   index differently.
//! * .NET matches over **UTF-16 code units**; fancy-regex matches over
//!   Unicode scalars. Every character outside the Basic Multilingual Plane is
//!   two units there and one scalar here, so `.`, a negated class, a
//!   quantifier count and the text of a captured group all differ over astral
//!   input. `^.$` over `"😀"` is "no" in .NET and "yes" here; `^..$` and
//!   `^.{2}$` are "yes" there and "no" here; and `^(.).*$` captures a lone
//!   high surrogate in .NET where group 1 is the whole emoji here. Nothing
//!   detects this, and `substr` counting code units does not help: the two
//!   extensions measure strings differently on purpose, each after its own
//!   .NET counterpart.
//! * `\w` and `\b` cover different characters. .NET defines `\w` as
//!   `[\p{L}\p{Mn}\p{Nd}\p{Pc}]`; the `regex` crate underneath fancy-regex
//!   defines it as `[\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}]`. Letter
//!   numbers (`Nl`, such as `Ⅷ`) and the `Mc` and `Me` marks are word
//!   characters here and not in .NET, and `\b` moves with them: `^\w+$` over
//!   `"Ⅷ"` is "no" there and "yes" here, and `\bx\b` over `"ⅧxⅧ"` is "yes"
//!   there and "no" here. `\d` and `\s` do agree.
//! * Case-insensitive matching folds differently, and setting
//!   [`RegexOptions::CULTURE_INVARIANT`] does not reconcile it. .NET compares
//!   the simple case *mapping* of two characters and never folds across a
//!   surrogate pair; fancy-regex uses Unicode simple case *folding*, which has
//!   larger equivalence classes. With `IgnoreCase | CultureInvariant` on both
//!   sides, `^s$` matches `"ſ"` here and not in .NET, `^σ$` matches `"ς"`, and
//!   `^𐐀$` matches `"𐐨"`. (Kelvin sign against `k`, and `ẞ` against `ß`, do
//!   agree.) Separately, .NET folds with the *thread* culture unless
//!   `CultureInvariant` is set, where this port is always culture-invariant —
//!   which is why the option is accepted and changes nothing.
//! * Character classes read differently in four ways, none of them
//!   detectable. .NET has class subtraction and no class intersection, no
//!   POSIX class names and no nesting; fancy-regex is the other way round.
//!     * `[a-z-[aeiou]]` is "a-z except the vowels" in .NET and the *union* of
//!       `a-z`, `-` and `aeiou` here, so `^[a-z-[aeiou]]+$` matches `"bad"`
//!       here and not there.
//!     * `[a&&b]` is the three characters `a`, `&`, `b` in .NET and the
//!       (empty) intersection of `a` and `b` here. So `[a-z&&[^aeiou]]`, the
//!       fancy-regex spelling of the subtraction above, is a one-way door: it
//!       means something else again in .NET.
//!     * `[[:alpha:]]` is `[`, `:`, `a`, `l`, `p`, `h` followed by a literal
//!       `]` in .NET, and the POSIX letter class here.
//!     * `[a[bc]]` is `a`, `[`, `b`, `c` followed by a literal `]` in .NET,
//!       and a nested union here.
//! * `\0` is the NUL character in .NET. fancy-regex reads it as a back
//!   reference to group 0: with the pinned 0.14 it compiles into a pattern
//!   that matches nothing at all, so `^\0$` over a NUL is "yes" in .NET and
//!   "no" here — the one divergence in this list that a later version of the
//!   engine turns loud (0.19 rejects the pattern). Write `\x00`.
//! * `a{,3}` is a literal `a{,3}` in .NET, which has no open-ended lower
//!   bound; fancy-regex reads it as `a{0,3}`. Write `a{0,3}`.
//! * .NET aborts a match that runs for 500 ms; fancy-regex has no timeout, so
//!   [`IsMatchFormatter::backtrack_limit`] guards runaway backtracking by step
//!   count instead. Exceeding it fails the format call.
//! * The message of a pattern that does not compile is fancy-regex's, not
//!   .NET's `ArgumentException` text.
//!
//! Everything else .NET accepts and fancy-regex does not — `(?'name'…)`, `\Z`,
//! balancing groups such as `(?<-x>…)`, block names such as
//! `\p{IsBasicLatin}`, octal escapes such as `\101` — fails the format call
//! with the engine's parse error rather than matching something else.
//!
//! [fancy-regex]: https://docs.rs/fancy-regex

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use fancy_regex::{Regex, RegexBuilder, RuntimeError};

#[cfg(feature = "time")]
use crate::fmt::date;
use crate::fmt::number::{self, Number};
use crate::formatter::{Formatter, FormattingInfo};
use crate::parsing::{Format, FormatItem};
use crate::value::Value;
use crate::Error;

use super::{split_format, split_part, InvalidSplitChar, DEFAULT_SPLIT_CHAR};

/// The default formatter name, .NET `IsMatchFormatter.Name`.
const NAME: &str = "ismatch";

/// The default name of the "magic" placeholder that reads the capturing
/// groups, .NET `IsMatchFormatter.PlaceholderNameForMatches`.
const DEFAULT_PLACEHOLDER_NAME_FOR_MATCHES: &str = "m";

/// fancy-regex's own default backtracking budget, which the port keeps as its
/// stand-in for .NET's 500 ms match timeout.
const DEFAULT_BACKTRACK_LIMIT: usize = 1_000_000;

// ---------------------------------------------------------------------------
// RegexOptions
// ---------------------------------------------------------------------------

/// The .NET `System.Text.RegularExpressions.RegexOptions` flags, with the same
/// names and the same bit values, so a template ported from C# can be given
/// the same options.
///
/// Not all of them survive the change of engine. Each is one of three kinds:
///
/// | Option | Here |
/// | --- | --- |
/// | [`IGNORE_CASE`](Self::IGNORE_CASE), [`MULTILINE`](Self::MULTILINE), [`SINGLELINE`](Self::SINGLELINE), [`IGNORE_PATTERN_WHITESPACE`](Self::IGNORE_PATTERN_WHITESPACE) | honored, as the fancy-regex inline flags `i`, `m`, `s`, `x` |
/// | [`COMPILED`](Self::COMPILED), [`CULTURE_INVARIANT`](Self::CULTURE_INVARIANT) | accepted and ignored |
/// | [`EXPLICIT_CAPTURE`](Self::EXPLICIT_CAPTURE), [`RIGHT_TO_LEFT`](Self::RIGHT_TO_LEFT), [`ECMA_SCRIPT`](Self::ECMA_SCRIPT), [`NON_BACKTRACKING`](Self::NON_BACKTRACKING) | rejected: the format call fails |
///
/// The rejected four all change which text a pattern matches, and fancy-regex
/// has nothing to map them to, so honoring them "as best we can" would mean
/// silently matching the wrong thing.
///
/// [`COMPILED`](Self::COMPILED) is a .NET code-generation hint with no effect
/// on what matches. [`CULTURE_INVARIANT`](Self::CULTURE_INVARIANT) is accepted
/// because this port is always culture-invariant: fancy-regex folds case by
/// Unicode simple case folding whatever the culture of the format call.
///
/// ```
/// use smartformat::extensions::ismatch::RegexOptions;
///
/// let options = RegexOptions::IGNORE_CASE | RegexOptions::MULTILINE;
/// assert!(options.contains(RegexOptions::IGNORE_CASE));
/// assert!(!options.contains(RegexOptions::SINGLELINE));
/// assert_eq!(options.bits(), 3);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct RegexOptions(u32);

impl RegexOptions {
    /// No options, the .NET default.
    pub const NONE: Self = Self(0);
    /// Case-insensitive matching (fancy-regex `i`).
    pub const IGNORE_CASE: Self = Self(1);
    /// `^` and `$` match at line boundaries (fancy-regex `m`).
    pub const MULTILINE: Self = Self(2);
    /// Only explicitly named groups capture. **Rejected**: fancy-regex has no
    /// `n` flag, and dropping the option would renumber every group that
    /// `{m[i]}` reads.
    pub const EXPLICIT_CAPTURE: Self = Self(4);
    /// Compile the pattern to IL. **Ignored**: a .NET performance hint.
    pub const COMPILED: Self = Self(8);
    /// `.` matches `\n` too (fancy-regex `s`). Note that .NET's "Singleline"
    /// is what most engines call "dot matches newline", *not* the opposite of
    /// [`MULTILINE`](Self::MULTILINE).
    pub const SINGLELINE: Self = Self(16);
    /// Ignore unescaped whitespace outside character classes and allow `#`
    /// comments (fancy-regex `x`).
    pub const IGNORE_PATTERN_WHITESPACE: Self = Self(32);
    /// Search from right to left. **Rejected**: fancy-regex only searches
    /// forwards, and a right-to-left search reports different capturing
    /// groups.
    pub const RIGHT_TO_LEFT: Self = Self(64);
    /// ECMAScript-conforming matching. **Rejected**: it redefines `\d`, `\w`
    /// and `\s` to ASCII and changes backreference semantics.
    pub const ECMA_SCRIPT: Self = Self(256);
    /// Culture-invariant case folding. **Ignored**: always on here.
    pub const CULTURE_INVARIANT: Self = Self(512);
    /// Use .NET's non-backtracking engine. **Rejected**: it is a different
    /// matcher, one that refuses lookarounds and backreferences, so accepting
    /// it silently would accept patterns .NET rejects.
    pub const NON_BACKTRACKING: Self = Self(1024);

    /// The raw .NET flag bits.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The options of these raw .NET flag bits. A bit .NET does not define is
    /// kept as it is and rejected when a pattern is compiled.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Whether every flag of `other` is set here.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The fancy-regex inline flag group this maps to — `"(?is)"`, or `""`
    /// when there is nothing to set — or the .NET name of the first option
    /// that has no counterpart.
    fn inline_flags(self) -> Result<String, &'static str> {
        for (option, name) in UNSUPPORTED {
            if self.contains(option) {
                return Err(name);
            }
        }
        // A bit .NET never defined cannot be honored either, and is far more
        // likely to be a miscomputed `from_bits` than a deliberate choice.
        if self.0 & !KNOWN.0 != 0 {
            return Err(UNKNOWN_FLAG);
        }

        let mut flags = String::new();
        for (option, flag) in [
            (Self::IGNORE_CASE, 'i'),
            (Self::MULTILINE, 'm'),
            (Self::SINGLELINE, 's'),
            (Self::IGNORE_PATTERN_WHITESPACE, 'x'),
        ] {
            if self.contains(option) {
                flags.push(flag);
            }
        }

        Ok(if flags.is_empty() {
            flags
        } else {
            format!("(?{flags})")
        })
    }
}

/// The options that change what a pattern matches and that fancy-regex cannot
/// express, with the .NET names the error message quotes.
const UNSUPPORTED: [(RegexOptions, &str); 4] = [
    (RegexOptions::EXPLICIT_CAPTURE, "ExplicitCapture"),
    (RegexOptions::RIGHT_TO_LEFT, "RightToLeft"),
    (RegexOptions::ECMA_SCRIPT, "ECMAScript"),
    (RegexOptions::NON_BACKTRACKING, "NonBacktracking"),
];

/// What the error message calls a bit .NET gives no name to.
const UNKNOWN_FLAG: &str = "(unknown flag)";

/// Every bit .NET gives a name to.
const KNOWN: RegexOptions = RegexOptions(1 | 2 | 4 | 8 | 16 | 32 | 64 | 256 | 512 | 1024);

/// The .NET name of every flag, in bit order.
const NAMES: [(RegexOptions, &str); 10] = [
    (RegexOptions::IGNORE_CASE, "IgnoreCase"),
    (RegexOptions::MULTILINE, "Multiline"),
    (RegexOptions::EXPLICIT_CAPTURE, "ExplicitCapture"),
    (RegexOptions::COMPILED, "Compiled"),
    (RegexOptions::SINGLELINE, "Singleline"),
    (
        RegexOptions::IGNORE_PATTERN_WHITESPACE,
        "IgnorePatternWhitespace",
    ),
    (RegexOptions::RIGHT_TO_LEFT, "RightToLeft"),
    (RegexOptions::ECMA_SCRIPT, "ECMAScript"),
    (RegexOptions::CULTURE_INVARIANT, "CultureInvariant"),
    (RegexOptions::NON_BACKTRACKING, "NonBacktracking"),
];

impl std::ops::BitOr for RegexOptions {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for RegexOptions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// The .NET names of the flags that are set, so a failing assertion reads like
/// the C# it was ported from.
impl fmt::Debug for RegexOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            return f.write_str("None");
        }
        let mut first = true;
        let mut rest = self.0;
        for (option, name) in NAMES {
            if !self.contains(option) {
                continue;
            }
            rest &= !option.0;
            if !first {
                f.write_str(" | ")?;
            }
            first = false;
            f.write_str(name)?;
        }
        if rest != 0 {
            if !first {
                f.write_str(" | ")?;
            }
            write!(f, "{rest}")?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// IsMatchFormatter
// ---------------------------------------------------------------------------

/// Renders one of two formats depending on whether a regular expression
/// matches the value, ported from .NET `IsMatchFormatter`.
///
/// The regular expression is the formatter's options — `^.+123.+$` in
/// `{0:ismatch(^.+123.+$):yes|no}` — read with SmartFormat's option escaping
/// resolved, so `\(`, `\)`, `\{`, `\}`, `\:` and `\\` reach the regex engine
/// as the single characters they name. A regex escape such as `\(` therefore
/// has to be written `\\\(` in a template.
///
/// A placeholder has to name the formatter unless
/// [`set_can_auto_detect`](Self::set_can_auto_detect) turns auto-detection on
/// (.NET `CanAutoDetect`, which defaults to `false` here as it does there).
///
/// The pattern is compiled on every call, as .NET's `new Regex(…)` is.
#[derive(Debug, Clone)]
pub struct IsMatchFormatter {
    name: String,
    split_char: char,
    can_auto_detect: bool,
    regex_options: RegexOptions,
    placeholder_name_for_matches: String,
    backtrack_limit: usize,
}

impl IsMatchFormatter {
    /// A formatter named `ismatch`, splitting on `|`, with no regex options
    /// and `m` as the name of the capturing-group placeholder — the .NET
    /// defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            can_auto_detect: false,
            regex_options: RegexOptions::NONE,
            placeholder_name_for_matches: DEFAULT_PLACEHOLDER_NAME_FOR_MATCHES.to_owned(),
            backtrack_limit: DEFAULT_BACKTRACK_LIMIT,
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the two formats are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that the character it replaces can be
    /// used in the output — `{0:ismatch(a):|yes|~|no|}` with `~`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`](super::VALID_SPLIT_CHARS)
    /// are accepted; .NET throws an `ArgumentException` for anything else.
    pub fn set_split_char(&mut self, split_char: char) -> Result<(), InvalidSplitChar> {
        self.split_char = super::valid_split_char(split_char)?;
        Ok(())
    }

    /// Whether a placeholder that names no formatter may be handled here.
    /// Defaults to `false`, as .NET's settable `CanAutoDetect` does.
    pub fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    pub fn set_can_auto_detect(&mut self, can_auto_detect: bool) {
        self.can_auto_detect = can_auto_detect;
    }

    /// The options every pattern is compiled with (.NET `RegexOptions`,
    /// [`RegexOptions::NONE`] by default).
    pub fn regex_options(&self) -> RegexOptions {
        self.regex_options
    }

    pub fn set_regex_options(&mut self, regex_options: RegexOptions) {
        self.regex_options = regex_options;
    }

    /// The name of the placeholder that reads the capturing groups of a
    /// successful match, `m` by default (.NET `PlaceholderNameForMatches`).
    ///
    /// A placeholder in the "matched" format whose *first* selector is spelled
    /// exactly like this is evaluated against the list of group values instead
    /// of against the formatted value, so `{m[0]}` is the whole match and
    /// `{m[1]}` the first group. The comparison is ordinal, as .NET's is, and
    /// ignores
    /// [`SmartSettings::case_sensitive`](crate::SmartSettings::case_sensitive).
    pub fn placeholder_name_for_matches(&self) -> &str {
        &self.placeholder_name_for_matches
    }

    pub fn set_placeholder_name_for_matches(&mut self, name: impl Into<String>) {
        self.placeholder_name_for_matches = name.into();
    }

    /// How many backtracking steps a match may take before the format call
    /// fails, fancy-regex's `RegexBuilder::backtrack_limit` (1,000,000 by
    /// default).
    ///
    /// This stands in for the 500 ms timeout .NET compiles every
    /// `IsMatchFormatter` pattern with. Both guard the same hazard — a pattern
    /// whose backtracking explodes on hostile input — but they measure it
    /// differently, so a pattern .NET times out on is not necessarily one that
    /// fails here, or the other way round. The step count has the advantage of
    /// being deterministic: the same template and the same value either always
    /// fail or always render, whatever the machine.
    pub fn backtrack_limit(&self) -> usize {
        self.backtrack_limit
    }

    pub fn set_backtrack_limit(&mut self, backtrack_limit: usize) {
        self.backtrack_limit = backtrack_limit;
    }

    /// The compiled pattern, or the error .NET's `new Regex(…)` would throw as
    /// an `ArgumentException` — with fancy-regex's message, which is the only
    /// one we have.
    fn compile(&self, info: &FormattingInfo<'_>, pattern: &str) -> Result<Regex, Error> {
        let flags = self.regex_options.inline_flags().map_err(|option| {
            plain_error(
                info,
                &format!(
                    "RegexOptions.{option} has no equivalent in the regex engine of this port"
                ),
            )
        })?;

        // The flags go in front of the pattern rather than into the builder,
        // which only offers `case_insensitive`. An inline flag group at the
        // start of a pattern applies to all of it, alternation branches
        // included, and opens no capturing group, so the numbering `{m[i]}`
        // reads stays as the author wrote it.
        RegexBuilder::new(&format!("{flags}{pattern}"))
            .backtrack_limit(self.backtrack_limit)
            .build()
            .map_err(|error| {
                plain_error(
                    info,
                    &format!("Invalid regular expression \"{pattern}\": {error}"),
                )
            })
    }

    /// Renders the "matched" format, .NET's loop over `formats[0].Items`.
    ///
    /// .NET formats each top-level item of that part on its own, so that it
    /// can swap the value under a `{m…}` placeholder, and cuts each item out
    /// of the *whole* format rather than out of the part — which is what
    /// `parent` is.
    fn write_match(
        &self,
        info: &mut FormattingInfo<'_>,
        parent: &Format,
        matched: &Format,
        groups: Value,
    ) -> Result<(), Error> {
        let matches = Value::Map(BTreeMap::from([(
            self.placeholder_name_for_matches.clone(),
            groups,
        )]));

        for item in &matched.items {
            let child = parent
                .substring(item.start(), item.end())
                .map_err(|error| plain_error(info, &error.to_string()))?;

            // .NET compares `Selectors[0].RawText` with an ordinal `==`, so a
            // placeholder is magic only when the name is spelled exactly.
            let is_magic = match item {
                FormatItem::Placeholder(placeholder) => placeholder
                    .selectors
                    .first()
                    .is_some_and(|selector| selector.text == self.placeholder_name_for_matches),
                FormatItem::Literal(_) => false,
            };

            if is_magic {
                info.format_as_child(&child, &matches)?;
            } else {
                info.format_as_child(&child, info.current())?;
            }
        }

        Ok(())
    }
}

impl Default for IsMatchFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for IsMatchFormatter {
    fn name(&self) -> &str {
        &self.name
    }

    fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        // Cannot deal with null.
        let Some(value) = value_text(info.current(), info) else {
            // A list or a map has no .NET counterpart worth matching — .NET
            // matches the pattern against `System.Collections…List`1[…]` and
            // the like — so, as the default formatter does with the same
            // rendering, we fail loudly rather than match a CLR type name.
            if matches!(info.current(), Value::List(_) | Value::Map(_)) {
                return Err(plain_error(
                    info,
                    "Matching a list or a map with \"ismatch\" is not supported; select a value from it",
                ));
            }
            return Ok(false);
        };

        // .NET reads `FormatterOptions` before it splits, so options holding
        // an escape sequence that resolves to nothing fail first.
        let expression = info.formatter_options()?;

        // .NET splits before it counts the parts, so a split that throws is
        // the answer however many parts the formatter wanted.
        let formats = info
            .format()
            .map(|format| split_format(info, format, self.split_char))
            .transpose()?;

        // Check whether the arguments can be handled by this formatter.
        // (.NET also has a `formats.Count == 0` branch returning `true`, which
        // is unreachable: `Format.Split` never yields fewer than one part.)
        let formats = match formats {
            Some(formats) if formats.len() == 2 => formats,
            _ => {
                let name = &info.placeholder().formatter_name;
                // Auto detection calls just return a failure to evaluate.
                if name.is_empty() {
                    return Ok(false);
                }
                // .NET throws a plain `FormatException` when the formatter was
                // called explicitly, which the evaluator only wraps in a
                // `FormattingException` while rethrowing — so the message
                // carries no `Error parsing format string: …` envelope.
                return Err(plain_error(
                    info,
                    &format!("Formatter named '{name}' requires at least 2 format options."),
                ));
            }
        };

        let regex = self.compile(info, expression)?;
        let captures = regex.captures(&value).map_err(|error| {
            // The stand-in for .NET's `RegexMatchTimeoutException` names the
            // budget it ran out of, since that is the knob to turn.
            let issue = match error {
                fancy_regex::Error::RuntimeError(RuntimeError::BacktrackLimitExceeded) => format!(
                    "Regular expression \"{expression}\" exceeded the backtrack limit of {} steps",
                    self.backtrack_limit
                ),
                error => format!("Regular expression \"{expression}\" failed to match: {error}"),
            };
            plain_error(info, &issue)
        })?;

        let Some(captures) = captures else {
            // Output the "no match" part of the format.
            let no_match = split_part(info, &formats[1])?;
            info.format_as_child(no_match, info.current())?;
            return Ok(true);
        };

        // Match successful. .NET reads `match.Groups`, whose first entry is
        // the whole match and whose groups that did not participate are the
        // empty string.
        let groups = Value::List(
            (0..captures.len())
                .map(|index| {
                    Value::String(
                        captures
                            .get(index)
                            .map_or("", |group| group.as_str())
                            .to_owned(),
                    )
                })
                .collect(),
        );

        // `info.format()` is `Some`, or the split above would not have run.
        let parent = info.format().expect("the format that was split");
        let matched = split_part(info, &formats[0])?;
        self.write_match(info, parent, matched, groups)?;

        Ok(true)
    }
}

/// The text .NET matches the pattern against, `CurrentValue.ToString()`.
///
/// `None` is "no text to match": a null value, which .NET declines before it
/// does anything else, and the two values whose .NET rendering is a CLR type
/// name.
///
/// As in [`ChooseFormatter`](super::ChooseFormatter), the conversion uses the
/// culture of the format call where .NET uses the thread culture — the same
/// thing whenever the two agree.
fn value_text<'v>(value: &'v Value, info: &FormattingInfo<'_>) -> Option<Cow<'v, str>> {
    let culture = info.culture();
    match value {
        Value::Null | Value::List(_) | Value::Map(_) => None,
        Value::String(text) => Some(Cow::Borrowed(text.as_str())),
        // .NET `bool.ToString()`.
        Value::Bool(true) => Some(Cow::Borrowed("True")),
        Value::Bool(false) => Some(Cow::Borrowed("False")),
        // The empty specifier is always valid, so these cannot fail.
        Value::Int(value) => Some(Cow::Owned(
            number::format_number(Number::Int(*value), "", culture).unwrap_or_default(),
        )),
        Value::UInt(value) => Some(Cow::Owned(
            number::format_number(Number::UInt(*value), "", culture).unwrap_or_default(),
        )),
        Value::Float(value) => Some(Cow::Owned(
            number::format_number(Number::Float(*value), "", culture).unwrap_or_default(),
        )),
        #[cfg(feature = "time")]
        Value::DateTime(value) => Some(Cow::Owned(
            date::format_datetime(value, "", culture).unwrap_or_default(),
        )),
    }
}

/// An error .NET raises as a plain exception inside `TryEvaluateFormat` — and
/// the ones this port raises where .NET raises nothing — positioned where the
/// evaluator positions a formatter failure.
fn plain_error(info: &FormattingInfo<'_>, issue: &str) -> Error {
    info.plain_error(issue, info.error_position())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/IsMatchFormatterTests.cs`, plus the
    //! cases that pin the divergences of the regex engine. The C# cases that
    //! render the whole group list with `{m:list:| - }` name each group
    //! instead, since the `list` formatter lands on its own branch.

    use std::collections::BTreeMap;

    use super::*;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::settings::{CaseSensitivity, ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    fn smart_with_settings(settings: SmartSettings, formatter: IsMatchFormatter) -> SmartFormatter {
        let mut smart = SmartFormatter::new(settings);
        // Only the formatter under test and the always-last DefaultFormatter,
        // so these tests pin this formatter rather than the order of the
        // default registry.
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    /// The .NET default of `FormatErrorAction.ThrowError` — our
    /// [`ErrorAction::Error`] — as the C# fixture sets it.
    fn smart_with(formatter: IsMatchFormatter) -> SmartFormatter {
        smart_with_settings(
            SmartSettings {
                format_error_action: ErrorAction::Error,
                ..SmartSettings::default()
            },
            formatter,
        )
    }

    fn smart() -> SmartFormatter {
        smart_with(IsMatchFormatter::new())
    }

    fn with_options(options: RegexOptions) -> SmartFormatter {
        let mut formatter = IsMatchFormatter::new();
        formatter.set_regex_options(options);
        smart_with(formatter)
    }

    /// The `_variable` of the C# fixture: `{theValue}` is `Some123Content`.
    fn the_value() -> Value {
        Value::Map(BTreeMap::from([(
            "theValue".to_owned(),
            Value::from("Some123Content"),
        )]))
    }

    fn args(values: impl IntoIterator<Item = Value>) -> Value {
        Value::List(values.into_iter().collect())
    }

    fn format(template: &str) -> String {
        format_with(smart(), template, the_value())
    }

    fn format_with(smart: SmartFormatter, template: &str, args: Value) -> String {
        smart
            .format(template, &args)
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    /// The message and the index of the error the template fails with. The
    /// index is the one .NET puts in the `FormattingException` envelope, so
    /// both halves are worth pinning.
    fn error_of(smart: SmartFormatter, template: &str, args: Value) -> (String, usize) {
        match smart.format(template, &args) {
            Err(Error::Format { message, position } | Error::Escape { message, position }) => {
                (message, position)
            }
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    fn message_of(smart: SmartFormatter, template: &str, args: Value) -> String {
        error_of(smart, template, args).0
    }

    // -----------------------------------------------------------------------
    // Defaults
    // -----------------------------------------------------------------------

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = IsMatchFormatter::new();
        assert_eq!(formatter.name(), "ismatch");
        assert!(!formatter.can_auto_detect());
        assert_eq!(formatter.split_char(), '|');
        assert_eq!(formatter.regex_options(), RegexOptions::NONE);
        assert_eq!(formatter.placeholder_name_for_matches(), "m");
        assert_eq!(IsMatchFormatter::new().with_name("re").name(), "re");
    }

    #[test]
    fn split_char_is_validated() {
        let mut formatter = IsMatchFormatter::new();
        assert!(formatter.set_split_char('~').is_ok());
        assert_eq!(formatter.split_char(), '~');
        assert_eq!(formatter.set_split_char('x'), Err(InvalidSplitChar('x')));
        // A rejected character leaves the previous one in place.
        assert_eq!(formatter.split_char(), '~');
    }

    #[test]
    fn is_not_auto_detected_by_default() {
        // A placeholder that names no formatter is left to the default one,
        // which for a string ignores the specifier. (A `{…:(regex):a|b}` has
        // no formatter *name*, so the parentheses are part of the format, not
        // the formatter options — the same in .NET.)
        assert_eq!(format("{theValue:a|b}"), "Some123Content");
    }

    #[test]
    fn auto_detection_can_be_turned_on() {
        let mut formatter = IsMatchFormatter::new();
        formatter.set_can_auto_detect(true);
        assert!(formatter.can_auto_detect());
        // With no formatter name there are no options either, so the pattern
        // is empty and matches anything.
        assert_eq!(
            format_with(smart_with(formatter), "{theValue:a|b}", the_value()),
            "a"
        );
    }

    // -----------------------------------------------------------------------
    // Test_Formats_And_CaseSensitivity
    // -----------------------------------------------------------------------

    #[test]
    fn formats_and_case_sensitivity() {
        for (template, options, expected) in [
            (
                "{theValue:ismatch(^.+123.+$):Okay - {}|No match content}",
                RegexOptions::NONE,
                "Okay - Some123Content",
            ),
            (
                "{theValue:ismatch(^.+123.+$):Fixed content if match|No match content}",
                RegexOptions::NONE,
                "Fixed content if match",
            ),
            (
                "{theValue:ismatch(^.+999.+$):{}|No match content}",
                RegexOptions::NONE,
                "No match content",
            ),
            (
                "{theValue:ismatch(^.+123.+$):|Only content with no match}",
                RegexOptions::NONE,
                "",
            ),
            (
                "{theValue:ismatch(^.+999.+$):|Only content with no match}",
                RegexOptions::NONE,
                "Only content with no match",
            ),
            (
                "{theValue:ismatch(^SOME123.+$):Okay - {}|No match content}",
                RegexOptions::IGNORE_CASE,
                "Okay - Some123Content",
            ),
            (
                "{theValue:ismatch(^SOME123.+$):Okay - {}|No match content}",
                RegexOptions::NONE,
                "No match content",
            ),
        ] {
            assert_eq!(
                format_with(with_options(options), template, the_value()),
                expected,
                "{template:?} with {options:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Less_Than_2_Format_Options_Should_Throw
    // -----------------------------------------------------------------------

    #[test]
    fn a_format_that_is_not_two_parts_is_an_error() {
        // Probed against 3.6.1, which reports the same message — and the
        // indexes below — for one part, three parts, an empty format and no
        // format at all.
        for (template, index) in [
            ("{theValue:ismatch(^.+123.+$):Dummy content}", 29),
            ("{theValue:ismatch(^.+123.+$):a|b|c}", 29),
            ("{theValue:ismatch(^.+123.+$):}", 29),
            ("{theValue:ismatch(^.+123.+$)}", 28),
        ] {
            let (message, position) = error_of(smart(), template, the_value());
            assert_eq!(
                message, "Formatter named 'ismatch' requires at least 2 format options.",
                "{template:?}"
            );
            assert_eq!(position, index, "{template:?}");
        }
    }

    #[test]
    fn a_format_that_is_not_two_parts_is_declined_when_unnamed() {
        // With auto-detection on and no formatter name, the same format is
        // declined instead of failing, which lets the next formatter try.
        let mut formatter = IsMatchFormatter::new();
        formatter.set_can_auto_detect(true);
        assert_eq!(
            format_with(smart_with(formatter), "{theValue:one part}", the_value()),
            "Some123Content"
        );
    }

    // -----------------------------------------------------------------------
    // Test_With_Changed_SplitChar
    // -----------------------------------------------------------------------

    #[test]
    fn changed_split_char() {
        for (template, expected) in [
            (
                "{theValue:ismatch(^.+123.+$):|Has match for '{}'|~|No match|}",
                "|Has match for 'Some123Content'|",
            ),
            (
                "{theValue:ismatch(^.+999.+$):|Has match for '{}'|~|No match|}",
                "|No match|",
            ),
        ] {
            let mut formatter = IsMatchFormatter::new();
            formatter.set_split_char('~').unwrap();
            assert_eq!(
                format_with(smart_with(formatter), template, the_value()),
                expected,
                "{template:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Match_And_List_Matches / Change_Placeholder_Name_For_Matches
    // -----------------------------------------------------------------------

    #[test]
    fn match_group_values() {
        // `\(` and `\)` are SmartFormat option escapes, so the pattern the
        // engine sees is `^.+(1)(2)(3).+$` — three capturing groups.
        assert_eq!(
            format(
                r"{theValue:ismatch(^.+\(1\)\(2\)\(3\).+$):Matches for '{}'\: {m[0]} - {m[1]} - {m[2]} - {m[3]}|No match}"
            ),
            "Matches for 'Some123Content': Some123Content - 1 - 2 - 3"
        );
        assert_eq!(
            format(r"{theValue:ismatch(^.+\(9\)\(9\)\(9\).+$):Matches for '{}'\: {m[1]}|No match}"),
            "No match"
        );
        assert_eq!(
            format(
                r"{theValue:ismatch(^.+\(1\)\(2\)\(3\).+$):First 2 matches in '{}'\: {m[1]} and {m[2]}|No match}"
            ),
            "First 2 matches in 'Some123Content': 1 and 2"
        );
    }

    #[test]
    fn placeholder_name_for_matches_can_be_changed() {
        let mut formatter = IsMatchFormatter::new();
        formatter.set_placeholder_name_for_matches("match");
        assert_eq!(formatter.placeholder_name_for_matches(), "match");
        let value = Value::Map(BTreeMap::from([(
            "theValue".to_owned(),
            Value::from("12345"),
        )]));
        assert_eq!(
            format_with(
                smart_with(formatter),
                r"{theValue:ismatch(^\(123\)\(4\)\(5\)$):First match in '{}'\: {match[1]}|No match}",
                value,
            ),
            "First match in '12345': 123"
        );
    }

    #[test]
    fn a_renamed_matches_placeholder_leaves_the_old_name_ordinary() {
        // With the name changed, `{m…}` is no longer magic: `m` is looked up
        // in the scope like any other selector, and there is no such key.
        let mut formatter = IsMatchFormatter::new();
        formatter.set_placeholder_name_for_matches("match");
        let message = message_of(
            smart_with(formatter),
            r"{theValue:ismatch(\(123\)):{m[1]}|no}",
            the_value(),
        );
        assert!(message.contains("selector named \"m\""), "{message}");
    }

    #[test]
    fn the_matches_name_is_compared_ordinally() {
        // .NET compares `Selectors[0].RawText` with `==`, so a case-insensitive
        // `SmartSettings` does not make `{M[1]}` magic.
        let smart = smart_with_settings(
            SmartSettings {
                format_error_action: ErrorAction::Error,
                case_sensitive: CaseSensitivity::CaseInsensitive,
                ..SmartSettings::default()
            },
            IsMatchFormatter::new(),
        );
        let message = message_of(smart, r"{theValue:ismatch(\(123\)):{M[1]}|no}", the_value());
        assert!(message.contains("selector named \"M\""), "{message}");
    }

    #[test]
    fn group_zero_is_the_whole_match() {
        assert_eq!(format(r"{theValue:ismatch(123):[{m[0]}]|no}"), "[123]");
    }

    #[test]
    fn a_group_that_did_not_participate_is_empty() {
        assert_eq!(
            format(r"{theValue:ismatch(\(123\)|\(zzz\)):[{m[1]}][{m[2]}]|no}"),
            "[123][]"
        );
    }

    #[test]
    fn an_out_of_range_group_is_a_selector_error() {
        let message = message_of(smart(), r"{theValue:ismatch(123):{m[5]}|no}", the_value());
        assert!(message.contains("selector named \"5\""), "{message}");
    }

    #[test]
    fn the_whole_group_list_needs_a_formatter_that_renders_a_list() {
        // .NET writes `System.Collections.Generic.List`1[System.String]` for a
        // bare `{m}`; the default formatter of this port refuses to render a
        // list at all, which is the divergence it documents.
        let message = message_of(smart(), r"{theValue:ismatch(123):{m}|no}", the_value());
        assert!(
            message.contains("Default formatting of a list is not supported"),
            "{message}"
        );
    }

    #[test]
    fn a_placeholder_that_is_not_the_matches_one_sees_the_value() {
        assert_eq!(
            format("{theValue:ismatch(123):got {} and {theValue}|no}"),
            "got Some123Content and Some123Content"
        );
    }

    // -----------------------------------------------------------------------
    // Currency_Symbol / Escaped_Option_And_RegEx_Chars
    // -----------------------------------------------------------------------

    #[test]
    fn currency_symbol() {
        // `\p{Sc}` escaped for the formatter options, exactly as the C# test's
        // `EscapedLiteral.EscapeCharLiterals` escapes it.
        for (currency, expected) in [
            ("€ Euro", "Currency: €"),
            ("¥ Yen", "Currency: ¥"),
            ("none", "Unknown"),
        ] {
            let value = Value::Map(BTreeMap::from([(
                "Currency".to_owned(),
                Value::from(currency),
            )]));
            assert_eq!(
                format_with(
                    smart(),
                    r"{Currency:ismatch(\\p\{Sc\}):Currency: {m[0]}|Unknown}",
                    value,
                ),
                expected,
                "{currency:?}"
            );
        }
    }

    #[test]
    fn escaped_option_and_regex_chars() {
        // (the character to find, the regex escape of it escaped again for the
        // formatter options) — the C# table verbatim.
        for (search, options) in [
            ("|", r"\\|"),
            ("?", r"\\?"),
            ("+", r"\\+"),
            ("*", r"\\*"),
            ("^", r"\\^"),
            ("$", r"\\$"),
            (".", r"\\."),
            ("[", r"\\["),
            ("]", r"\\]"),
            (":", r"\:"),
            ("\\", r"\\\\"),
            ("(", r"\\\("),
            (")", r"\\\)"),
            ("{", r"\\\{"),
            ("}", r"\\\}"),
        ] {
            let template = std::format!("{{0:ismatch({options}):found {{}}|}}");
            assert_eq!(
                format_with(smart(), &template, args([Value::from(search)])),
                std::format!("found {search}"),
                "{template:?}"
            );
        }
    }

    #[test]
    fn match_special_characters() {
        // (options as the C# test escapes them, input, whether it matches)
        for (options, input, should_match) in [
            (r"\\\(\([^\\\)]*\)\\\)", "Text (inside) parenthesis", true),
            (r"\\\(\([^\\\)]*\)\\\)", "No parenthesis", false),
            (r"Lon\(?=don\)", "This is London", true),
            (r"Lon\(?=don\)", "This is Loando", false),
            ("<[^<>]+>", "<abcde>", true),
            ("<[^<>]+>", "<>", false),
            (r"\\d\{3,\}", "1234", true),
            (r"\\d\{3,\}", "12", false),
            (r"^.\{5,\}\:,$", "1z%aW:,", true),
            (r"^.\{5,\}\:,$", "1z:,", false),
        ] {
            let template = std::format!("{{0:ismatch({options}):found {{}}|}}");
            let expected = if should_match {
                std::format!("found {input}")
            } else {
                String::new()
            };
            assert_eq!(
                format_with(smart(), &template, args([Value::from(input)])),
                expected,
                "{template:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Values other than strings
    // -----------------------------------------------------------------------

    #[test]
    fn non_string_values_match_their_text() {
        assert_eq!(
            format_with(
                smart(),
                "{0:ismatch(True):yes|no}",
                args([Value::Bool(true)])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^1\\.5$):yes|no}",
                args([Value::Float(1.5)])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\(1\)2$):a {m[1]}|b}",
                args([Value::Int(12)])
            ),
            "a 1"
        );
    }

    #[test]
    fn a_null_value_is_declined() {
        // .NET returns `false` for a null value, which leaves the placeholder
        // with no formatter at all.
        let message = message_of(smart(), "{0:ismatch(^.+$):a|b}", args([Value::Null]));
        assert!(
            message.contains("No suitable Formatter could be found"),
            "{message}"
        );
    }

    #[test]
    fn a_list_value_fails_loudly() {
        // .NET would match the pattern against `System.Collections…List`1[…]`;
        // the default formatter refuses the same rendering.
        let message = message_of(
            smart(),
            "{0:ismatch(System):a|b}",
            args([Value::List(vec![Value::Int(1)])]),
        );
        assert!(message.contains("is not supported"), "{message}");
    }

    // -----------------------------------------------------------------------
    // Options and the format
    // -----------------------------------------------------------------------

    #[test]
    fn an_unresolvable_option_escape_fails() {
        // .NET throws from the `FormatterOptions` getter, before it splits.
        let message = message_of(
            smart(),
            r"{0:ismatch(a\qb):yes|no}",
            args([Value::from("x")]),
        );
        assert_eq!(message, "Unrecognized escape sequence \"\\q\" in literal.");
    }

    #[test]
    fn an_empty_pattern_matches_everything() {
        assert_eq!(
            format_with(
                smart(),
                "{0:ismatch():yes [{m[0]}]|no}",
                args([Value::from("abc")])
            ),
            "yes []"
        );
    }

    #[test]
    fn the_alignment_applies_to_every_item() {
        // .NET formats each item of the matched part as its own child, each
        // padded to the placeholder's alignment. Probed against 3.6.1.
        assert_eq!(
            format_with(smart(), "{0,10:ismatch(b):Y|N}", args([Value::from("abc")])),
            "         Y"
        );
    }

    // -----------------------------------------------------------------------
    // RegexOptions
    // -----------------------------------------------------------------------

    #[test]
    fn honored_regex_options() {
        assert_eq!(
            format_with(
                with_options(RegexOptions::MULTILINE),
                "{0:ismatch(^b$):yes|no}",
                args([Value::from("a\nb\nc")])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                smart(),
                "{0:ismatch(^b$):yes|no}",
                args([Value::from("a\nb\nc")])
            ),
            "no"
        );
        assert_eq!(
            format_with(
                with_options(RegexOptions::SINGLELINE),
                "{0:ismatch(^a.b$):yes|no}",
                args([Value::from("a\nb")])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                with_options(RegexOptions::IGNORE_PATTERN_WHITESPACE),
                "{0:ismatch(^a b c$):yes|no}",
                args([Value::from("abc")])
            ),
            "yes"
        );
        // An inline flag group at the start covers every alternation branch,
        // as .NET's option does.
        assert_eq!(
            format_with(
                with_options(RegexOptions::IGNORE_CASE),
                "{0:ismatch(^A|B$):yes|no}",
                args([Value::from("b")])
            ),
            "yes"
        );
        // Accepted and ignored.
        assert_eq!(
            format_with(
                with_options(RegexOptions::COMPILED | RegexOptions::CULTURE_INVARIANT),
                "{0:ismatch(^abc$):yes|no}",
                args([Value::from("abc")])
            ),
            "yes"
        );
    }

    #[test]
    fn unsupported_regex_options_fail_loudly() {
        for (options, name) in [
            (RegexOptions::EXPLICIT_CAPTURE, "ExplicitCapture"),
            (RegexOptions::RIGHT_TO_LEFT, "RightToLeft"),
            (RegexOptions::ECMA_SCRIPT, "ECMAScript"),
            (RegexOptions::NON_BACKTRACKING, "NonBacktracking"),
            (RegexOptions::from_bits(1 << 20), UNKNOWN_FLAG),
        ] {
            let message = message_of(
                with_options(options | RegexOptions::IGNORE_CASE),
                "{0:ismatch(^abc$):yes|no}",
                args([Value::from("abc")]),
            );
            assert_eq!(
                message,
                std::format!(
                    "RegexOptions.{name} has no equivalent in the regex engine of this port"
                )
            );
        }
    }

    #[test]
    fn regex_options_debug_reads_like_dotnet() {
        assert_eq!(std::format!("{:?}", RegexOptions::NONE), "None");
        assert_eq!(
            std::format!(
                "{:?}",
                RegexOptions::IGNORE_CASE | RegexOptions::CULTURE_INVARIANT
            ),
            "IgnoreCase | CultureInvariant"
        );
        assert_eq!(
            std::format!("{:?}", RegexOptions::from_bits(1 << 20)),
            "1048576"
        );
        let mut options = RegexOptions::NONE;
        options |= RegexOptions::SINGLELINE;
        assert_eq!(options, RegexOptions::SINGLELINE);
    }

    // -----------------------------------------------------------------------
    // Divergences from .NET's regex engine
    // -----------------------------------------------------------------------

    #[test]
    fn a_pattern_dotnet_accepts_and_fancy_regex_does_not_fails_loudly() {
        // Each of these is valid .NET syntax with no fancy-regex counterpart.
        // The point of the test is that none of them quietly matches something
        // else. (Probed against 3.6.1: .NET renders "yes" for all four.)
        for options in [
            // (?'name'…), .NET's alternative named-group syntax
            r"\(?'x'a\)b",
            // \Z, "end of input, or just before a final newline"
            r"^ab\\Z",
            // (?<-x>…), a balancing group
            r"^\(?<x>a\)\(?<-x>b\)$",
            // \p{IsBasicLatin}, a .NET Unicode block name
            r"^\\p\{IsBasicLatin\}+$",
        ] {
            let template = std::format!("{{0:ismatch({options}):yes|no}}");
            let message = message_of(smart(), &template, args([Value::from("ab")]));
            assert!(
                message.starts_with("Invalid regular expression"),
                "{template:?}: {message}"
            );
        }
    }

    #[test]
    fn dotnet_syntax_that_both_engines_agree_on() {
        // The counterweight to the test above: these are .NET constructs the
        // `regex` crate alone would reject, and they behave the same in both.
        // (Probed against 3.6.1: .NET renders "yes" for all of them.)
        for (options, input) in [
            // lookahead
            (r"Lon\(?=don\)", "This is London"),
            // backreference
            (r"\(a\)\\1", "aa"),
            // named backreference
            (r"\(?<x>a\)\\k<x>", "aa"),
            // atomic group
            (r"\(?>a+\)b", "ab"),
            // inline comment
            (r"a\(?#hi\)b", "ab"),
            // conditional
            (r"^\(a\)?\(?\(1\)b|c\)$", "ab"),
        ] {
            let template = std::format!("{{0:ismatch({options}):yes|no}}");
            assert_eq!(
                format_with(smart(), &template, args([Value::from(input)])),
                "yes",
                "{template:?}"
            );
        }
    }

    #[test]
    fn dollar_does_not_match_before_a_final_newline() {
        // .NET renders "yes" here: its `$` also matches just before a trailing
        // `\n`. fancy-regex anchors at the very end, and no flag changes it.
        assert_eq!(
            format_with(
                smart(),
                "{0:ismatch(^abc$):yes|no}",
                args([Value::from("abc\n")])
            ),
            "no"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^abc\\z):yes|no}",
                args([Value::from("abc")])
            ),
            "yes"
        );
    }

    #[test]
    fn named_groups_are_numbered_left_to_right() {
        // .NET numbers named groups after the unnamed ones and renders
        // "[a][c][b]"; fancy-regex numbers them where they stand.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(\(a\)\(?<x>b\)\(c\)):[{m[1]}][{m[2]}][{m[3]}]|no}",
                args([Value::from("abc")])
            ),
            "[a][b][c]"
        );
    }

    #[test]
    fn ignore_pattern_whitespace_stops_at_a_character_class() {
        // Both engines keep whitespace inside a character class significant,
        // so this agrees with .NET rather than diverging from it.
        assert_eq!(
            format_with(
                with_options(RegexOptions::IGNORE_PATTERN_WHITESPACE),
                "{0:ismatch(^[a b]+$):yes|no}",
                args([Value::from("a b")])
            ),
            "yes"
        );
    }

    #[test]
    fn an_open_lower_bound_is_a_silent_divergence() {
        // .NET has no `{,n}` quantifier and reads `a{,3}` as the five literal
        // characters; fancy-regex reads it as `a{0,3}`. Probed against 3.6.1,
        // which renders "no" for "aa" and "yes" for the literal text.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^a\{,3\}$):yes|no}",
                args([Value::from("aa")])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^a\{,3\}$):yes|no}",
                args([Value::from("a{,3}")])
            ),
            "no"
        );
    }

    #[test]
    fn character_class_subtraction_is_a_silent_divergence() {
        // .NET reads `[a-z-[aeiou]]` as "a-z except the vowels" and renders
        // "no" for "bad"; fancy-regex reads it as a union and renders "yes".
        // Both engines accept the pattern, so the port cannot detect this and
        // it is pinned here rather than only described in the docs.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^[a-z-[aeiou]]+$):yes|no}",
                args([Value::from("bad")])
            ),
            "yes"
        );
        // The fancy-regex spelling of the same intent works as expected —
        // here. It is a one-way door: see the test below.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^[a-z&&[^aeiou]]+$):yes|no}",
                args([Value::from("bad")])
            ),
            "no"
        );
    }

    #[test]
    fn class_intersection_posix_names_and_nesting_are_silent_divergences() {
        // Three more character-class constructs both engines accept and read
        // differently. Probed against 3.6.1, which renders the opposite of
        // every assertion here: .NET has no intersection, no POSIX class name
        // and no nesting, so it reads each of these as a set of literal
        // characters followed by a literal `]`.
        for (pattern, input, here) in [
            // .NET: the three characters `a`, `&`, `b`. Here: the empty
            // intersection of `a` and `b`, which matches nothing.
            (r"^[a&&b]+$", "ab", "no"),
            (r"^[a&&b]+$", "&", "no"),
            // .NET: `[`, `:`, `a`, `l`, `p`, `h` and then a literal `]`.
            (r"^[[\:alpha\:]]+$", "abc", "yes"),
            // .NET: `a`, `[`, `b`, `c` and then a literal `]`.
            (r"^[a[bc]]+$", "abc", "yes"),
        ] {
            let template = std::format!("{{0:ismatch({pattern}):yes|no}}");
            assert_eq!(
                format_with(smart(), &template, args([Value::from(input)])),
                here,
                "{template:?} over {input:?}"
            );
        }
    }

    #[test]
    fn matching_counts_scalars_where_dotnet_counts_utf16_units() {
        // .NET's engine runs over UTF-16 code units, so an astral character is
        // two of them; fancy-regex runs over Unicode scalars, where it is one.
        // Probed against 3.6.1, which renders the opposite of each of the
        // first four.
        let emoji = || args([Value::from("\u{1F600}")]);
        assert_eq!(
            format_with(smart(), r"{0:ismatch(^.$):yes|no}", emoji()),
            "yes"
        );
        assert_eq!(
            format_with(smart(), r"{0:ismatch(^..$):yes|no}", emoji()),
            "no"
        );
        assert_eq!(
            format_with(smart(), r"{0:ismatch(^.\{2\}$):yes|no}", emoji()),
            "no"
        );
        // A negated class is one scalar here and one code unit in .NET, so it
        // matches the whole emoji here and only half of it there.
        assert_eq!(
            format_with(smart(), r"{0:ismatch(^[^a]$):yes|no}", emoji()),
            "yes"
        );
        // A captured group carries the whole character here; in .NET group 1
        // is the lone high surrogate, which its UTF-8 encoding writes as
        // U+FFFD.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\(.\).*$):[{m[1]}]|no}",
                args([Value::from("\u{1F600}abc")])
            ),
            "[\u{1F600}]"
        );
    }

    #[test]
    fn the_word_character_class_covers_more_than_dotnets() {
        // .NET's `\w` is `[\p{L}\p{Mn}\p{Nd}\p{Pc}]`; the `regex` crate's is
        // `[\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}]`. Probed against
        // 3.6.1, which renders "no" for the first three and "yes" for the
        // fourth.
        for input in [
            "\u{2167}", // Ⅷ, a letter number (Nl)
            "\u{903}",  // Devanagari visarga, a spacing mark (Mc)
            "a\u{488}", // combining cyrillic hundred thousands (Me)
        ] {
            assert_eq!(
                format_with(
                    smart(),
                    r"{0:ismatch(^\\w+$):yes|no}",
                    args([Value::from(input)])
                ),
                "yes",
                "{input:?}"
            );
        }
        // `\b` moves with `\w`: between `Ⅷ` and `x` there is a boundary in
        // .NET and none here.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(\\bx\\b):yes|no}",
                args([Value::from("\u{2167}x\u{2167}")])
            ),
            "no"
        );
        // `\d` and `\s` do agree: an Arabic-Indic digit and NEL match in both.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\\d+$):yes|no}",
                args([Value::from("\u{663}")])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\\s$):yes|no}",
                args([Value::from("\u{85}")])
            ),
            "yes"
        );
    }

    #[test]
    fn case_insensitive_matching_folds_wider_than_dotnets() {
        // `CultureInvariant` does not reconcile the two: .NET compares simple
        // case *mappings* and never folds across a surrogate pair, where
        // fancy-regex uses Unicode simple case *folding*. Probed against 3.6.1
        // with the same two options set, which renders "no" for all three.
        let folding = || with_options(RegexOptions::IGNORE_CASE | RegexOptions::CULTURE_INVARIANT);
        for (pattern, input) in [
            ("^s$", "\u{17f}"),           // ſ, long s
            ("^\u{3c3}$", "\u{3c2}"),     // σ against final ς
            ("^\u{10400}$", "\u{10428}"), // Deseret, a surrogate pair in .NET
        ] {
            let template = std::format!("{{0:ismatch({pattern}):yes|no}}");
            assert_eq!(
                format_with(folding(), &template, args([Value::from(input)])),
                "yes",
                "{template:?} over {input:?}"
            );
        }
        // The controls that do agree, so the test is about folding width and
        // not about `IgnoreCase` at large.
        for (pattern, input, both) in [
            ("^k$", "\u{212a}", "yes"),      // Kelvin sign
            ("^\u{1e9e}$", "\u{df}", "yes"), // ẞ against ß
            ("^i$", "\u{131}", "no"),        // dotless ı
            ("^i$", "\u{130}", "no"),        // dotted İ
        ] {
            let template = std::format!("{{0:ismatch({pattern}):yes|no}}");
            assert_eq!(
                format_with(folding(), &template, args([Value::from(input)])),
                both,
                "{template:?} over {input:?}"
            );
        }
    }

    #[test]
    fn a_nul_escape_matches_nothing_and_an_octal_escape_fails() {
        // `\0` is NUL in .NET, which renders "yes" for both of these (probed).
        // fancy-regex 0.14 reads it as a back reference to group 0 and builds
        // a pattern that matches nothing at all — the only silent divergence
        // in this module that a dependency bump would turn loud, since 0.19
        // rejects the same pattern.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\\0$):yes|no}",
                args([Value::from("\0")])
            ),
            "no"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(a\\0b):yes|no}",
                args([Value::from("a\0b")])
            ),
            "no"
        );
        // `\x00` is the spelling that works in both.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\\x00$):yes|no}",
                args([Value::from("\0")])
            ),
            "yes"
        );
        // An octal escape is loud rather than silent: .NET renders "yes" for
        // `^\101$` over "A" and fancy-regex refuses to compile it.
        let message = message_of(
            smart(),
            r"{0:ismatch(^\\101$):yes|no}",
            args([Value::from("A")]),
        );
        assert!(
            message.starts_with("Invalid regular expression"),
            "{message}"
        );
    }

    #[test]
    fn lookaround_and_backreferences_work() {
        // The two features fancy-regex adds over the `regex` crate, and the
        // reason it is the engine this port uses.
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(Lon\(?=don\)):yes|no}",
                args([Value::from("This is London")])
            ),
            "yes"
        );
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(\(a\)\\1):yes|no}",
                args([Value::from("aa")])
            ),
            "yes"
        );
    }

    #[test]
    fn a_runaway_pattern_hits_the_backtrack_limit() {
        // `(a+)+` over a long run of `a`s that cannot match is the classic
        // exponential backtrack; the lookahead in front of it keeps
        // fancy-regex from handing the whole pattern to the linear-time
        // `regex` engine, which would not backtrack at all. .NET would abort
        // this after 500 ms; here the step budget cuts it off, and does so
        // deterministically.
        let mut formatter = IsMatchFormatter::new();
        formatter.set_backtrack_limit(1000);
        assert_eq!(formatter.backtrack_limit(), 1000);
        let message = message_of(
            smart_with(formatter),
            r"{0:ismatch(^\(?=a\)\(a+\)+$):yes|no}",
            args([Value::String("a".repeat(30) + "b")]),
        );
        assert_eq!(
            message,
            "Regular expression \"^(?=a)(a+)+$\" exceeded the backtrack limit of 1000 steps"
        );
    }

    #[test]
    fn the_default_backtrack_limit_is_fancy_regexs() {
        // A pattern that stays well inside the budget renders normally, so the
        // guard costs nothing on ordinary templates.
        assert_eq!(IsMatchFormatter::new().backtrack_limit(), 1_000_000);
        assert_eq!(
            format_with(
                smart(),
                r"{0:ismatch(^\(?=a\)\(a+\)+$):yes|no}",
                args([Value::String("a".repeat(30))])
            ),
            "yes"
        );
    }
}
