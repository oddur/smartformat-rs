//! The characters with a special meaning to the parser.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/Core/Settings/ParserSettings.cs`.

/// Starts an escape sequence. SmartFormat.NET allows no other escape character.
pub(crate) const CHAR_LITERAL_ESCAPE_CHAR: char = '\\';
/// Separates the selectors from the formatter name, and the name from the format.
pub(crate) const FORMATTER_NAME_SEPARATOR: char = ':';
/// Introduces the alignment, as in `{name,10}`.
pub(crate) const ALIGNMENT_OPERATOR: char = ',';
/// Marks a selector as nullable, as in `{First?.Second}`.
pub(crate) const NULLABLE_OPERATOR: char = '?';
pub(crate) const PLACEHOLDER_BEGIN_CHAR: char = '{';
pub(crate) const PLACEHOLDER_END_CHAR: char = '}';
pub(crate) const FORMATTER_OPTIONS_BEGIN_CHAR: char = '(';
pub(crate) const FORMATTER_OPTIONS_END_CHAR: char = ')';
/// Ends a list index, as in `{Numbers[0]}`.
pub(crate) const LIST_INDEX_END_CHAR: char = ']';

/// Terminate the parsing of formatter options unless escaped.
pub(crate) const FORMAT_OPTIONS_TERMINATOR_CHARS: [char; 5] = [
    FORMATTER_NAME_SEPARATOR,
    FORMATTER_OPTIONS_BEGIN_CHAR,
    FORMATTER_OPTIONS_END_CHAR,
    PLACEHOLDER_BEGIN_CHAR,
    PLACEHOLDER_END_CHAR,
];

/// Split selectors from each other. Contiguous operator characters form one operator.
pub(crate) const OPERATOR_CHARS: [char; 5] = [
    '.',
    NULLABLE_OPERATOR,
    ALIGNMENT_OPERATOR,
    '[',
    LIST_INDEX_END_CHAR,
];

/// Delimit a selector; they can never be part of one.
pub(crate) const SELECTOR_DELIMITING_CHARS: [char; 5] = [
    FORMATTER_NAME_SEPARATOR,
    PLACEHOLDER_BEGIN_CHAR,
    PLACEHOLDER_END_CHAR,
    FORMATTER_OPTIONS_BEGIN_CHAR,
    FORMATTER_OPTIONS_END_CHAR,
];

/// The allowlist used by [`SelectorFilter::Alphanumeric`](super::SelectorFilter::Alphanumeric).
pub(crate) const STANDARD_ALLOWLIST: &str =
    "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_-";

/// The 68 non-visual characters blocked by
/// [`SelectorFilter::VisualUnicodeChars`](super::SelectorFilter::VisualUnicodeChars).
pub(crate) const NON_VISUAL_UNICODE_CHARACTERS: [char; 68] = [
    // Control characters (U+0000–U+001F, U+007F)
    '\u{0}', '\u{1}', '\u{2}', '\u{3}', '\u{4}', '\u{5}', '\u{6}', '\u{7}', '\u{8}', '\u{9}',
    '\u{a}', '\u{b}', '\u{c}', '\u{d}', '\u{e}', '\u{f}', '\u{10}', '\u{11}', '\u{12}', '\u{13}',
    '\u{14}', '\u{15}', '\u{16}', '\u{17}', '\u{18}', '\u{19}', '\u{1a}', '\u{1b}', '\u{1c}',
    '\u{1d}', '\u{1e}', '\u{1f}', '\u{7f}', // Format characters (category Cf)
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}',
    // Directional formatting (category Cf)
    '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}', // Invisible separator
    '\u{2063}', // Common combining marks (category Mn)
    '\u{300}', '\u{301}', '\u{302}', '\u{308}',
    // Whitespace characters (non-glyph spacing)
    '\u{a0}', '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}',
    '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}', '\u{202f}', '\u{205f}', '\u{3000}',
];
