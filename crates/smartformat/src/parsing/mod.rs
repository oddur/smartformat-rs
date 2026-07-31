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
