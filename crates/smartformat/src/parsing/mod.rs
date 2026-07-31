//! The template parser and its syntax tree.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/Core/Parsing/`. The .NET types
//! keep index pairs into the input string and materialize substrings lazily;
//! here every node owns its strings, and additionally carries the byte range it
//! was parsed from so the engine can reproduce the original tokens.

pub(crate) mod chars;
mod escaped_literal;
mod parser;
mod settings;

#[cfg(test)]
mod tests;

pub use parser::Parser;
pub use settings::{CustomCharError, ParserSettings, SelectorFilter};

use std::fmt;
use std::fmt::Write as _;

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
    pub fn split(&self, separator: char) -> Vec<Format> {
        let positions = self.find_all(separator);
        // .NET returns the format itself when it holds no separator.
        if positions.is_empty() {
            return vec![self.clone()];
        }

        let mut pieces = Vec::with_capacity(positions.len() + 1);
        let mut start = self.start;
        for position in positions {
            pieces.push(self.substring(start, position));
            start = position + separator.len_utf8();
        }
        pieces.push(self.substring(start, self.end));
        pieces
    }

    /// Every offset of `separator` in the literal text of this format, as a
    /// byte offset into the template (.NET `Format.FindAll` over
    /// `Format.IndexOf`).
    fn find_all(&self, separator: char) -> Vec<usize> {
        let mut positions = Vec::new();
        for item in &self.items {
            // .NET searches the *source* text of a literal, so a separator that
            // is part of an escape sequence — `\|`, which is not a valid
            // sequence and fails when it is written — splits the format all the
            // same.
            if let FormatItem::Literal(literal) = item {
                positions.extend(
                    literal
                        .raw
                        .match_indices(separator)
                        .map(|(offset, _)| literal.start + offset),
                );
            }
        }
        positions
    }

    /// The part of this format between two byte offsets into the template
    /// (.NET `Format.Substring`).
    ///
    /// A placeholder that reaches into the range is taken whole, since a
    /// placeholder cannot be split; a literal is sliced. Offsets outside this
    /// format's range are clamped rather than rejected, where .NET throws
    /// `ArgumentOutOfRangeException` — no caller can reach that, since the
    /// offsets always come from [`split`](Self::split) or from a match inside
    /// the format's own text.
    pub fn substring(&self, start: usize, end: usize) -> Format {
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

        Format {
            raw: slice(&self.raw, self.start, start, end),
            items,
            start,
            end,
        }
    }
}

/// The part of a literal between two byte offsets into the template.
///
/// The escape sequences of the slice are resolved afresh, as .NET resolves
/// those of a `Format.Substring` slice: a slice that cuts one in half is
/// resolved as the truncated sequence it now is, so the left half of
/// `\u00|41` is `\u00`, which is a NUL character and not four characters of
/// text. (.NET's `\u` accepts fewer than four hex digits at the end of a
/// slice.) The common case, a split character inside `\|`, is a lone `\` on
/// the left, which resolves to itself either way.
fn slice_literal(literal: &LiteralText, start: usize, end: usize) -> LiteralText {
    if start == literal.start && end == literal.end {
        return literal.clone();
    }

    let raw = slice(&literal.raw, literal.start, start, end);
    let convert = literal.convert_character_literals;
    let chars: Vec<char> = raw.chars().collect();
    let (text, escape_error) = match escaped_literal::resolve_literal(&chars, convert) {
        Ok(text) => (text, None),
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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
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
    /// Byte offset one past the last character in the input.
    pub end: usize,
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
