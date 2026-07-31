//! Formatter extensions beyond `DefaultFormatter`, each a port of the
//! same-named SmartFormat.NET extension. They implement
//! [`Formatter`](crate::formatter::Formatter) and are registered by
//! `SmartFormatter` in .NET's `CreateDefaultSmartFormat` order.

pub mod choose;
pub mod conditional;
#[cfg(feature = "plural")]
pub mod plural;
#[cfg(feature = "plural")]
pub mod plural_rules;

use std::fmt;

pub use choose::ChooseFormatter;
pub use conditional::ConditionalFormatter;
#[cfg(feature = "plural")]
pub use plural::PluralLocalizationFormatter;

/// The characters .NET accepts as the split character of a formatter that
/// reads a list of parts (`Utilities.Validation.GetValidSplitCharOrThrow`).
///
/// [`ChooseFormatter`], [`ConditionalFormatter`] and
/// `PluralLocalizationFormatter` all validate against this list.
pub const VALID_SPLIT_CHARS: [char; 3] = ['|', ',', '~'];

/// The .NET default split character.
pub(crate) const DEFAULT_SPLIT_CHAR: char = VALID_SPLIT_CHARS[0];

/// A split character that is not one of [`VALID_SPLIT_CHARS`] was passed to a
/// formatter's `set_split_char` (.NET `ArgumentException`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSplitChar(pub char);

impl fmt::Display for InvalidSplitChar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Only '{}', '{}' and '{}' are valid split chars.",
            VALID_SPLIT_CHARS[0], VALID_SPLIT_CHARS[1], VALID_SPLIT_CHARS[2]
        )
    }
}

impl std::error::Error for InvalidSplitChar {}

/// `split_char` if [`VALID_SPLIT_CHARS`] holds it, an error otherwise.
pub(crate) fn valid_split_char(split_char: char) -> Result<char, InvalidSplitChar> {
    if VALID_SPLIT_CHARS.contains(&split_char) {
        Ok(split_char)
    } else {
        Err(InvalidSplitChar(split_char))
    }
}
