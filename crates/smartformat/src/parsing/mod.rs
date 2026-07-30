//! Template parser, ported from SmartFormat.NET's `Core/Parsing`
//! (`Parser.cs`, `Format.cs`, `Placeholder.cs`, `Selector.cs`,
//! `EscapedLiteral.cs`).
//!
//! The AST below is the contract between the parser and the formatting
//! engine; keep it stable.

use crate::error::Error;

/// A parsed template: a flat list of literal and placeholder items.
#[derive(Debug, Clone, PartialEq)]
pub struct Format {
    pub items: Vec<FormatItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FormatItem {
    /// Literal text with escape sequences already resolved.
    Literal(String),
    Placeholder(Placeholder),
}

/// `{Selectors,Alignment:FormatterName(Options):NestedFormat}`
#[derive(Debug, Clone, PartialEq)]
pub struct Placeholder {
    pub selectors: Vec<Selector>,
    /// `{0,10}` → 10, `{0,-10}` → -10, no alignment → 0.
    pub alignment: i32,
    /// Explicit formatter name (`{0:plural:...}` → `"plural"`); empty when
    /// the placeholder has no named formatter.
    pub formatter_name: String,
    /// Formatter options in parens (`{0:choose(m|f):...}` → `"m|f"`).
    pub formatter_options: String,
    /// The format after the (first) `:`, if any. May contain nested
    /// placeholders.
    pub format: Option<Format>,
    /// Byte offset of the opening `{` in the source template, for errors.
    pub position: usize,
}

/// One step of a selector chain: in `{Person?.Name}`, `Person` (operator ``)
/// then `Name` (operator `?.`).
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    pub text: String,
    /// The operator preceding this selector: `""`, `"."`, `"?."`, or `","`
    /// (alignment). Mirrors SmartFormat's `ParserSettings.OperatorChars`.
    pub operator: String,
}

/// Parser configuration, mirroring SmartFormat.NET's `ParserSettings`
/// defaults (char literals enabled, `\`-escaping, not string.Format
/// compatibility mode).
#[derive(Debug, Clone, Default)]
pub struct ParserSettings {
    /// .NET `ParserSettings.ConvertCharacterStringLiterals` (default true):
    /// resolve `\n`, `\t`, `\\`, `\{`, `\}` etc. in literal text.
    pub convert_character_string_literals: bool,
}

#[derive(Debug, Default)]
pub struct Parser {
    pub settings: ParserSettings,
}

impl Parser {
    pub fn new(settings: ParserSettings) -> Self {
        Self { settings }
    }

    /// Parses a template into a [`Format`]. Error recovery per
    /// `ErrorAction` is applied by the caller; this returns all syntax
    /// errors found.
    pub fn parse(&self, template: &str) -> Result<Format, Error> {
        let _ = template;
        todo!("milestone M1: port Parser.cs")
    }
}
