//! Parser settings, mirroring SmartFormat.NET's `ParserSettings`.

use std::collections::HashSet;
use std::fmt;

use super::chars::{
    CHAR_LITERAL_ESCAPE_CHAR, NON_VISUAL_UNICODE_CHARACTERS, OPERATOR_CHARS,
    SELECTOR_DELIMITING_CHARS, STANDARD_ALLOWLIST,
};
use crate::settings::ErrorAction;

/// Which characters a selector may consist of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorFilter {
    /// Allowlist: ASCII letters and digits plus `_` and `-`, and any custom
    /// selector characters. This is the .NET default.
    Alphanumeric,
    /// Blocklist: every Unicode character except the 68 non-visual ones, the
    /// operator characters, the escape character and the delimiting characters.
    VisualUnicodeChars,
}

/// Settings for [`Parser`](super::Parser), mirroring SmartFormat.NET's `ParserSettings`.
///
/// Settings are read when the [`Parser`](super::Parser) is created; later changes
/// have no effect on an existing parser, as in .NET.
#[derive(Debug, Clone)]
pub struct ParserSettings {
    /// How parsing errors are handled. .NET default: `ParseErrorAction.ThrowError`.
    pub error_action: ErrorAction,
    /// When `true` (the .NET default), `\n`, `\t`, `•` … in literal text are
    /// converted to the characters they stand for. When `false` they stay verbatim.
    pub convert_character_string_literals: bool,
    /// Which characters are accepted inside a selector.
    pub selector_char_filter: SelectorFilter,
    /// When `true`, braces are escaped `string.Format`-style by doubling them
    /// (`{{` and `}}`) and formatter names are not parsed.
    pub string_format_compatibility: bool,
    /// Characters allowed inside selectors on top of the
    /// [`selector_char_filter`](Self::selector_char_filter). Prefer
    /// [`add_custom_selector_chars`](Self::add_custom_selector_chars), which
    /// rejects characters that already have a meaning.
    pub custom_selector_chars: Vec<char>,
    /// Characters that separate selectors on top of the standard `. ? , [ ]`.
    /// Prefer [`add_custom_operator_chars`](Self::add_custom_operator_chars),
    /// which rejects characters that already have a meaning.
    pub custom_operator_chars: Vec<char>,
}

// The .NET defaults, which differ from what `#[derive(Default)]` would produce:
// `ConvertCharacterStringLiterals` is `true` in SmartFormat.NET.
impl Default for ParserSettings {
    fn default() -> Self {
        Self {
            error_action: ErrorAction::Error,
            convert_character_string_literals: true,
            selector_char_filter: SelectorFilter::Alphanumeric,
            string_format_compatibility: false,
            custom_selector_chars: Vec::new(),
            custom_operator_chars: Vec::new(),
        }
    }
}

/// A character was rejected as a custom selector or operator character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomCharError {
    /// The rejected character.
    pub character: char,
    /// Why it was rejected.
    pub message: String,
}

impl fmt::Display for CustomCharError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}': {}", self.character, self.message)
    }
}

impl std::error::Error for CustomCharError {}

impl ParserSettings {
    /// Allows additional characters inside selectors, on top of the ones the
    /// [`selector_char_filter`](Self::selector_char_filter) permits.
    pub fn add_custom_selector_chars(
        &mut self,
        characters: impl IntoIterator<Item = char>,
    ) -> Result<(), CustomCharError> {
        for c in characters {
            if SELECTOR_DELIMITING_CHARS.contains(&c)
                || c == CHAR_LITERAL_ESCAPE_CHAR
                || OPERATOR_CHARS.contains(&c)
                || self.custom_operator_chars.contains(&c)
            {
                return Err(CustomCharError {
                    character: c,
                    message: "cannot be a custom selector character: it is disallowed or in use as an operator character".to_owned(),
                });
            }

            if NON_VISUAL_UNICODE_CHARACTERS.contains(&c) {
                self.custom_selector_chars.push(c);
            }

            if self.selector_char_filter == SelectorFilter::Alphanumeric
                && !(STANDARD_ALLOWLIST.contains(c) || self.custom_selector_chars.contains(&c))
            {
                self.custom_selector_chars.push(c);
            }
        }
        Ok(())
    }

    /// Allows additional characters to separate selectors. Contiguous operator
    /// characters are parsed as one operator.
    pub fn add_custom_operator_chars(
        &mut self,
        characters: impl IntoIterator<Item = char>,
    ) -> Result<(), CustomCharError> {
        for c in characters {
            if SELECTOR_DELIMITING_CHARS.contains(&c) || self.custom_selector_chars.contains(&c) {
                return Err(CustomCharError {
                    character: c,
                    message: "cannot be a custom operator character: it is disallowed or in use as a selector character".to_owned(),
                });
            }

            if !OPERATOR_CHARS.contains(&c) && !self.custom_operator_chars.contains(&c) {
                self.custom_operator_chars.push(c);
            }
        }
        Ok(())
    }

    pub(crate) fn operator_chars(&self) -> HashSet<char> {
        OPERATOR_CHARS
            .iter()
            .copied()
            .chain(self.custom_operator_chars.iter().copied())
            .collect()
    }

    pub(crate) fn selector_chars(&self) -> CharSet {
        match self.selector_char_filter {
            SelectorFilter::Alphanumeric => CharSet {
                chars: STANDARD_ALLOWLIST
                    .chars()
                    .chain(self.custom_selector_chars.iter().copied())
                    .collect(),
                is_allow_list: true,
            },
            SelectorFilter::VisualUnicodeChars => {
                let mut chars: HashSet<char> = [CHAR_LITERAL_ESCAPE_CHAR]
                    .into_iter()
                    .chain(SELECTOR_DELIMITING_CHARS)
                    .chain(OPERATOR_CHARS)
                    .chain(self.custom_operator_chars.iter().copied())
                    .chain(NON_VISUAL_UNICODE_CHARACTERS)
                    .collect();
                for c in &self.custom_selector_chars {
                    chars.remove(c);
                }
                CharSet {
                    chars,
                    is_allow_list: false,
                }
            }
        }
    }
}

/// An allowlist or a blocklist of characters.
#[derive(Debug)]
pub(crate) struct CharSet {
    chars: HashSet<char>,
    is_allow_list: bool,
}

impl CharSet {
    pub(crate) fn is_allowed(&self, c: char) -> bool {
        self.chars.contains(&c) == self.is_allow_list
    }
}
