//! The template parser and its syntax tree.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/Core/Parsing/`. The .NET types
//! keep index pairs into the input string and materialize substrings lazily;
//! here every node owns its strings, and additionally carries the byte range it
//! was parsed from so the engine can reproduce the original tokens.

pub(crate) mod chars;
pub(crate) mod escaped_literal;
mod parser;
mod settings;

#[cfg(test)]
mod tests;

pub(crate) use escaped_literal::is_dotnet_white;
pub use parser::Parser;
pub use settings::{CustomCharError, ParserSettings, SelectorFilter};

use std::fmt;
use std::fmt::Write as _;

use crate::dotnet_messages;

/// The `ArgumentOutOfRangeException` .NET throws while splitting a format that
/// holds a literal whose ends are crossed — the ones the parser leaves behind
/// when it reads past the end of a `\uXXXX` sequence that is not four hex
/// digits, as in `{0:cond:a|\u12}`.
///
/// Such a literal's source text reaches past the end of the format it belongs
/// to, so the search for the separators can both ask for a negative count
/// ([`Count`](Self::Count)) and report a separator the format does not cover,
/// which then makes `Format.Substring` cut a piece out of bounds
/// ([`Start`](Self::Start), [`Length`](Self::Length)).
///
/// The three differ in *when* they are raised, which a template can see:
/// [`Count`](Self::Count) comes out of `Format.IndexOf`, which runs before any
/// piece exists, so it fails the whole split and every argument with it — not
/// even a piece count the formatter would have rejected is reached. The other
/// two come out of `Format.Substring`, which .NET defers until a formatter asks
/// for that one piece, so only the argument that picks the bad piece fails; see
/// [`Format::split`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// .NET asked `string.IndexOf(char, int, int)` for a negative count while
    /// it was still looking for the separators (`Format.IndexOf`).
    Count,
    /// A piece starts outside the format it is cut from
    /// (`Format.Substring`'s `nameof(start)`).
    Start,
    /// A piece ends past the end of the format it is cut from
    /// (`Format.Substring`'s `nameof(length)`).
    Length,
}

impl SplitError {
    /// The exception message, .NET's verbatim: it is what
    /// [`ErrorAction::OutputErrorInResult`](crate::ErrorAction::OutputErrorInResult)
    /// writes into the result, so a reworded copy would be a rendering
    /// difference.
    pub const fn message(self) -> &'static str {
        match self {
            SplitError::Count => "Count must be positive and count must refer to a location within the string/array/collection. (Parameter 'count')",
            SplitError::Start => dotnet_messages::OUT_OF_RANGE_START,
            SplitError::Length => dotnet_messages::OUT_OF_RANGE_LENGTH,
        }
    }
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for SplitError {}

/// One piece of a [`Format::split`]: the piece itself, or the
/// [`SplitError`] .NET's `Format.Substring` throws when it is asked to cut a
/// piece the format does not cover.
///
/// The error is kept per piece rather than failing the split because .NET's
/// `SplitList` is lazy: it only records where the separators are and cuts each
/// piece out when the formatter indexes it. So
/// `{0:choose(1|2|3):a|b|\u12}` renders `a` for 1 and `b` for 2, and only 3 —
/// the argument that picks the piece cut out of bounds — fails.
pub type SplitPiece = Result<Format, SplitError>;

/// A parsed format string: literal text interleaved with placeholders.
///
/// A [`Placeholder`] may hold a nested `Format`, which is the part after the
/// formatter name — `one|two|three` in `{Items:choose(1|2|3):one|two|three}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Format {
    /// Literal text and placeholders, in the order they appear.
    pub items: Vec<FormatItem>,
    /// The input text this format spans, unchanged (.NET `Format.AsSpan()`).
    /// For the format returned by [`Parser::parse`] that is the whole
    /// template, which is what error messages quote.
    pub raw: String,
    /// Byte offset of the first character of this format in the input.
    pub start: usize,
    /// Byte offset one past the last character of this format in the input.
    pub end: usize,
}

impl Format {
    /// Whether this format contains at least one nested [`Placeholder`].
    pub fn has_nested(&self) -> bool {
        self.items
            .iter()
            .any(|item| matches!(item, FormatItem::Placeholder(_)))
    }

    /// The literal text of this format with escape sequences resolved,
    /// excluding the text of any placeholder. A sequence that resolves to
    /// nothing stays as written; see [`LiteralText::escape_error`].
    pub fn literal_text(&self) -> String {
        let mut result = String::new();
        for item in &self.items {
            if let FormatItem::Literal(literal) = item {
                result.push_str(&literal.text);
            }
        }
        result
    }

    /// The message of the first literal whose escape sequence resolves to
    /// nothing, with the byte offset that literal starts at.
    ///
    /// .NET resolves the escape sequences while it concatenates a format's
    /// items — `LiteralText.AsSpan()` inside `Format.ToString()` — and throws
    /// at the first one that resolves to nothing. So anything that builds a
    /// format's text without writing it, `Format.RawText` and
    /// `Format.GetLiteralText` alike, asks this first.
    ///
    /// What the message becomes, and where it is reported, is the caller's:
    /// the formatters that do this deliberately differ, some raising an escape
    /// error and some a plain one, some at the literal and some at the
    /// placeholder's [`error_position`]. This only finds the failing literal.
    ///
    /// [`error_position`]: crate::formatter::FormattingInfo::error_position
    pub fn first_escape_error(&self) -> Option<(&str, usize)> {
        self.items.iter().find_map(|item| match item {
            FormatItem::Literal(literal) => literal
                .escape_error
                .as_deref()
                .map(|message| (message, literal.start)),
            FormatItem::Placeholder(_) => None,
        })
    }

    /// Splits this format on `separator` at the top nesting level, the way the
    /// `choose`, `cond` and `plural` formatters read their parts
    /// (.NET `Format.Split` over `SplitList`).
    ///
    /// The separator is only looked for in literal text, so a nested
    /// placeholder is never cut in half and neither is the text inside it:
    /// `a|{0:b|c}|d` splits into `a`, `{0:b|c}` and `d`. A format that holds no
    /// separator splits into itself, so the result is never empty and
    /// `split(…).len() - 1` is the number of separators found.
    ///
    /// Each piece keeps the byte range and the raw source text it covers, so a
    /// piece can be rendered — and reported on — exactly like the format it
    /// came from.
    ///
    /// A format the parser left with a literal whose ends are crossed — see
    /// [`LiteralText::end`] — is where the two halves of the return type come
    /// in, since .NET throws at either of two moments. Looking for the
    /// separators can ask for a negative count, which throws out of the whole
    /// split
    /// ([`SplitError::Count`], the outer `Err`); or it can report a separator
    /// past the end of this format, which only throws when the piece it cuts
    /// is asked for ([`SplitError::Start`] or [`SplitError::Length`], a
    /// [`SplitPiece`] that is `Err`). .NET's `SplitList` cuts lazily, so a
    /// formatter that never picks the bad piece never sees the second kind.
    pub fn split(&self, separator: char) -> Result<Vec<SplitPiece>, SplitError> {
        self.split_max(separator, usize::MAX)
    }

    /// [`split`](Self::split) with a limit on the number of separators, which
    /// is .NET's `Format.Split(char, maxCount)` — the overload only
    /// `ListFormatter` calls, with a limit of four.
    ///
    /// The limit is not a formality. .NET stops searching once it has found
    /// `max_count` separators, so a literal whose ends are crossed *after* the
    /// last one it needs is never reached and never fails:
    /// `{0:list:{}|-|+|*|x\u12}` renders `a-b+c` in 3.6.1 (probed), where an
    /// unlimited split fails the placeholder with [`SplitError::Count`].
    /// Whatever follows the last separator still becomes one final piece, so
    /// this returns up to `max_count + 1` of them.
    pub fn split_max(
        &self,
        separator: char,
        max_count: usize,
    ) -> Result<Vec<SplitPiece>, SplitError> {
        let positions = self.find_all(separator, max_count)?;
        // .NET returns the format itself when it holds no separator.
        if positions.is_empty() {
            return Ok(vec![Ok(self.clone())]);
        }

        let mut pieces = Vec::with_capacity(positions.len() + 1);
        let mut start = self.start;
        for position in positions {
            pieces.push(self.substring(start, position));
            start = position + separator.len_utf8();
        }
        // .NET cuts the last piece as "everything left", so its end is this
        // format's end however far past it the last separator was.
        pieces.push(self.substring(start, self.end));
        Ok(pieces)
    }

    /// The first `max_count` offsets of `separator` in the literal text of this
    /// format, as byte offsets into the template (.NET `Format.FindAll` over
    /// `Format.IndexOf`), or the [`SplitError`] the search reached.
    fn find_all(&self, separator: char, max_count: usize) -> Result<Vec<usize>, SplitError> {
        let mut positions = Vec::new();
        let mut from = self.start;
        while positions.len() < max_count {
            let Some(position) = self.index_of(separator, from)? else {
                break;
            };
            positions.push(position);
            from = position + separator.len_utf8();
        }
        Ok(positions)
    }

    /// The first offset of `separator` at or after `from` (.NET
    /// `Format.IndexOf`), searching only literal text: a separator inside a
    /// nested placeholder never splits the format.
    ///
    /// .NET searches the *source* text of a literal, so a separator that is
    /// part of an escape sequence — `\|`, which is not a valid sequence and
    /// fails when it is written — splits the format all the same.
    ///
    /// A literal whose end is before the point the search has reached is where
    /// .NET asks `string.IndexOf` for a negative count and throws, so the
    /// search fails with [`SplitError`] rather than returning an offset. A
    /// crossed literal the search has *already passed* is skipped like any
    /// other item and is not an error.
    fn index_of(&self, separator: char, from: usize) -> Result<Option<usize>, SplitError> {
        let mut start = from;
        for item in &self.items {
            // Note the strict `<`: a literal ending exactly where the search
            // starts is searched, with a count of zero.
            if item.end() < start {
                continue;
            }
            let FormatItem::Literal(literal) = item else {
                continue;
            };

            let search_start = start.max(literal.start);
            if literal.end < search_start {
                return Err(SplitError::Count);
            }
            start = search_start;

            let offset = start - literal.start;
            if let Some(found) = literal
                .raw
                .get(offset..)
                .and_then(|rest| rest.find(separator))
            {
                return Ok(Some(start + found));
            }
        }
        Ok(None)
    }

    /// The part of this format between two byte offsets into the template
    /// (.NET `Format.Substring`).
    ///
    /// A placeholder that reaches into the range is taken whole, since a
    /// placeholder cannot be split; a literal is sliced.
    ///
    /// Offsets outside this format's range are an error, .NET's
    /// `Format.Substring` validating its arguments in that order: a `start`
    /// before the format or past its end is [`SplitError::Start`], and an `end`
    /// past the format's end is [`SplitError::Length`], after .NET's parameter
    /// names. [`split`](Self::split) reaches both, because a separator found in
    /// a literal whose ends are crossed can lie past this format's end.
    pub fn substring(&self, start: usize, end: usize) -> Result<Format, SplitError> {
        // .NET `Format.ValidateArguments`, `start` before `length`.
        if start < self.start || start > self.end {
            return Err(SplitError::Start);
        }
        if end > self.end {
            return Err(SplitError::Length);
        }

        let mut items = Vec::new();
        for item in &self.items {
            if item.end() <= start {
                continue; // Skip the items before the substring.
            }
            if end <= item.start() {
                break; // Done.
            }

            match item {
                FormatItem::Literal(literal) => items.push(FormatItem::Literal(slice_literal(
                    literal,
                    literal.start.max(start),
                    literal.end.min(end),
                ))),
                // A placeholder cannot be split, so one that reaches into the
                // substring is taken whole, as in .NET.
                placeholder => items.push(placeholder.clone()),
            }
        }

        Ok(Format {
            raw: slice(&self.raw, self.start, start, end),
            items,
            start,
            end,
        })
    }
}

/// The part of a literal between two byte offsets into the template.
///
/// The escape sequences of the slice are resolved afresh, as .NET resolves
/// those of a `Format.Substring` slice: a slice that cuts one in half is
/// resolved as the truncated sequence it now is, so the left half of
/// `\u00|41` is `\u00`, which is a NUL character and not four characters of
/// text. (.NET's `\u` takes however many of the four characters the slice
/// still holds and parses those as hex, so one to three digits are fine —
/// but none at all, the left half of `\u|abcd`, is an error.) The common
/// case, a split character inside `\|`, is a lone `\` on the left, which
/// resolves to itself either way.
fn slice_literal(literal: &LiteralText, start: usize, end: usize) -> LiteralText {
    if start == literal.start && end == literal.end {
        return literal.clone();
    }

    LiteralText::resolved(
        slice(&literal.raw, literal.start, start, end),
        start,
        end,
        literal.convert_character_literals,
    )
}

/// `text[start..end]`, where all three offsets are byte offsets into the
/// template and `text` is the source text starting at `base`.
fn slice(text: &str, base: usize, start: usize, end: usize) -> String {
    text.get(start.saturating_sub(base)..end.saturating_sub(base))
        .unwrap_or_default()
        .to_owned()
}

/// Reconstructs the format string, with escape sequences resolved but
/// placeholders kept verbatim.
impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for item in &self.items {
            match item {
                FormatItem::Literal(literal) => f.write_str(&literal.text)?,
                FormatItem::Placeholder(placeholder) => f.write_str(&placeholder.raw)?,
            }
        }
        Ok(())
    }
}

/// One item of a [`Format`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatItem {
    /// Text outside of `{braces}`.
    Literal(LiteralText),
    /// A `{placeholder}`.
    Placeholder(Placeholder),
}

impl FormatItem {
    /// Byte offset of the first character of this item in the input.
    pub fn start(&self) -> usize {
        match self {
            FormatItem::Literal(literal) => literal.start,
            FormatItem::Placeholder(placeholder) => placeholder.start,
        }
    }

    /// Byte offset one past the last character of this item in the input.
    pub fn end(&self) -> usize {
        match self {
            FormatItem::Literal(literal) => literal.end,
            FormatItem::Placeholder(placeholder) => placeholder.end,
        }
    }

    /// The text this item was parsed from, unchanged.
    pub fn raw(&self) -> &str {
        match self {
            FormatItem::Literal(literal) => &literal.raw,
            FormatItem::Placeholder(placeholder) => &placeholder.raw,
        }
    }
}

/// Literal text found in a format string.
///
/// The parser puts every escape sequence into a `LiteralText` of its own, so
/// [`text`](Self::text) is at most one character longer than the sequence it
/// resolves.
///
/// [`text`](Self::text) and [`escape_error`](Self::escape_error) are derived
/// from [`raw`](Self::raw) and
/// [`convert_character_literals`](Self::convert_character_literals), so the
/// struct is built through [`LiteralText::resolved`] rather than field by
/// field: there is no default for `convert_character_literals` that is right
/// on its own, and deriving `Default` would have to pick one that contradicts
/// [`ParserSettings::convert_character_string_literals`]. `#[non_exhaustive]`
/// keeps it that way for callers outside the crate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiteralText {
    /// The text with escape sequences resolved. An escape sequence that
    /// resolves to nothing is left as written and reported by
    /// [`escape_error`](Self::escape_error) instead.
    pub text: String,
    /// The text as it appears in the input.
    pub raw: String,
    /// Why the escape sequence in [`raw`](Self::raw) could not be resolved,
    /// if it could not. .NET resolves escape sequences when the literal is
    /// written, so the message only becomes an error if this literal is ever
    /// rendered — which a format that a formatter reads as a specifier, such
    /// as `{0:0.00}`, never is.
    pub escape_error: Option<String>,
    /// Whether the escape sequences in [`raw`](Self::raw) are resolved at all,
    /// the parser's
    /// [`convert_character_string_literals`](ParserSettings::convert_character_string_literals)
    /// setting. .NET reads the setting off the item every time it resolves
    /// (`LiteralText.AsSpan()`), so a slice of this literal — one of the pieces
    /// [`Format::split`](Format::split) cuts — is resolved the same way.
    pub convert_character_literals: bool,
    /// Byte offset of the first character in the input.
    pub start: usize,
    /// Byte offset one past the last character in the input. It can be *before*
    /// [`start`](Self::start): the parser reads past the end of a `\uXXXX`
    /// sequence whose four characters are not hex digits, and .NET leaves the
    /// text between there and wherever the literal really ended as a literal
    /// whose ends are crossed. Such a literal holds no text; it only ever
    /// shows up as a [`SplitError`], either the one
    /// [`Format::split`](Format::split) fails with when it reaches one or the
    /// one a piece cut past the end of the format carries.
    pub end: usize,
}

impl LiteralText {
    /// A literal spanning `start..end` of the input, its escape sequences
    /// resolved (.NET `LiteralText.AsSpan()`, which the port runs once here
    /// rather than on every write).
    ///
    /// `convert` is
    /// [`ParserSettings::convert_character_string_literals`], which .NET reads
    /// off the settings the item was created with. A sequence that resolves to
    /// nothing is not an error yet: it stays as written in
    /// [`text`](Self::text) and the reason lands in
    /// [`escape_error`](Self::escape_error), to be raised if the literal is
    /// ever written.
    pub fn resolved(raw: String, start: usize, end: usize, convert: bool) -> Self {
        let (text, escape_error) = match escaped_literal::resolve_literal(&raw, convert) {
            // Nothing to resolve, which is the common case: one clone.
            Ok(None) => (raw.clone(), None),
            Ok(Some(text)) => (text, None),
            Err(message) => (raw.clone(), Some(message)),
        };

        LiteralText {
            text,
            raw,
            escape_error,
            convert_character_literals: convert,
            start,
            end,
        }
    }
}

/// The part of a format string between `{` and `}`.
///
/// For `{Items.Length,-10:choose(1|2|3):one|two|three}` the
/// [`selectors`](Self::selectors) are `Items`, `Length` and `-10` (the last one
/// carrying the `,` operator), the [`alignment`](Self::alignment) is `-10`, the
/// [`formatter_name`](Self::formatter_name) is `choose`, the
/// [`formatter_options`](Self::formatter_options) are `1|2|3` and the
/// [`format`](Self::format) is `one|two|three`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Placeholder {
    /// The selector chain, in source order.
    pub selectors: Vec<Selector>,
    /// The alignment as in `string.Format("{0,-10}")`; `0` if there is none.
    /// Nested placeholders inherit the alignment of the placeholder they are in.
    pub alignment: i32,
    /// The formatter name, or empty if the placeholder has none.
    pub formatter_name: String,
    /// The formatter options with escape sequences resolved. An escape
    /// sequence that resolves to nothing is left as written and reported by
    /// [`formatter_options_error`](Self::formatter_options_error) instead.
    pub formatter_options: String,
    /// The formatter options as they appear in the input.
    pub formatter_options_raw: String,
    /// Why the escape sequences in
    /// [`formatter_options_raw`](Self::formatter_options_raw) could not be
    /// resolved, if they could not. .NET resolves them in the
    /// `Placeholder.FormatterOptions` getter, so the message only becomes an
    /// error if a formatter reads the options.
    pub formatter_options_error: Option<String>,
    /// The format after the formatter name, if the placeholder has one.
    pub format: Option<Format>,
    /// The nesting level, starting at 1 for a top-level placeholder.
    pub nested_depth: usize,
    /// The text this placeholder was parsed from, including the braces.
    /// Used to put the tokens back when recovering from a parse error.
    pub raw: String,
    /// Byte offset of the opening brace in the input.
    pub start: usize,
    /// Byte offset one past the closing brace in the input.
    pub end: usize,
}

/// Reconstructs the placeholder from its parsed components.
impl fmt::Display for Placeholder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char(chars::PLACEHOLDER_BEGIN_CHAR)?;
        for selector in &self.selectors {
            // The alignment is appended below, in normalized form.
            if selector.operator.starts_with(chars::ALIGNMENT_OPERATOR) {
                continue;
            }
            f.write_str(&selector.operator)?;
            f.write_str(&selector.text)?;
        }
        if self.alignment != 0 {
            write!(f, "{}{}", chars::ALIGNMENT_OPERATOR, self.alignment)?;
        }
        if !self.formatter_name.is_empty() {
            write!(
                f,
                "{}{}",
                chars::FORMATTER_NAME_SEPARATOR,
                self.formatter_name
            )?;
            if !self.formatter_options.is_empty() {
                write!(
                    f,
                    "{}{}{}",
                    chars::FORMATTER_OPTIONS_BEGIN_CHAR,
                    self.formatter_options,
                    chars::FORMATTER_OPTIONS_END_CHAR
                )?;
            }
        }
        if let Some(format) = &self.format {
            // .NET writes `Format.AsSpan()` here, which is the untouched
            // source text, not the escape-resolved `Format.ToString()`.
            write!(f, "{}{}", chars::FORMATTER_NAME_SEPARATOR, format.raw)?;
        }
        f.write_char(chars::PLACEHOLDER_END_CHAR)
    }
}

/// One selector of a [`Placeholder`], e.g. `Second` in `{First?.Second}`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selector {
    /// The selector itself, without its operator.
    pub text: String,
    /// The operator that preceded the selector — `.`, `?.`, `[`, `].`, `,` … —
    /// or empty for the first selector of a placeholder.
    pub operator: String,
    /// The position of the selector within its placeholder, starting at 0.
    pub index: usize,
    /// Byte offset of the first character of [`text`](Self::text) in the input.
    pub start: usize,
    /// Byte offset one past the last character of [`text`](Self::text) in the input.
    pub end: usize,
}
