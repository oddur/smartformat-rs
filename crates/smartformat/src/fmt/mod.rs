//! .NET-compatible value formatting: standard format specifiers applied by
//! `DefaultFormatter` (the equivalent of .NET `IFormattable.ToString(spec)`).
//!
//! Standard specifiers only; custom patterns (`#,##0.00`, `yyyy-MM-dd`) are
//! rejected with [`FormatSpecError::Unsupported`] so compatibility gaps are
//! loud rather than silently wrong.

pub mod culture;
#[cfg(feature = "time")]
pub mod date;
pub mod number;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatSpecError {
    /// The spec is valid .NET but outside our supported subset
    /// (e.g. a custom pattern). Message names the offending spec.
    Unsupported(String),
    /// The spec is not valid .NET at all.
    Invalid(String),
}

impl fmt::Display for FormatSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatSpecError::Unsupported(s) => write!(f, "unsupported format spec: {s}"),
            FormatSpecError::Invalid(s) => write!(f, "invalid format spec: {s}"),
        }
    }
}

impl std::error::Error for FormatSpecError {}

/// The length of `text` in UTF-16 code units, which is the unit .NET counts
/// text in: `string.Length`, the alignment width `string.Format` pads to, and
/// every index a `FormattingException` reports.
pub(crate) fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}
