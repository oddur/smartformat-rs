//! Formatter extensions beyond `DefaultFormatter`, each a port of the
//! same-named SmartFormat.NET extension. They implement
//! [`Formatter`](crate::formatter::Formatter) and are registered by
//! `SmartFormatter` in .NET's `CreateDefaultSmartFormat` order.

pub mod choose;
pub mod conditional;
#[cfg(feature = "regex-formatters")]
pub mod ismatch;
pub mod list;
pub mod localization;
pub mod null;
#[cfg(feature = "plural")]
pub mod plural;
#[cfg(feature = "plural")]
pub mod plural_rules;
pub mod substring;
pub mod template;
#[cfg(feature = "time")]
pub mod time;

use std::fmt;

use crate::formatter::FormattingInfo;
use crate::parsing::{Format, SplitPiece};
use crate::Error;

pub use choose::ChooseFormatter;
pub use conditional::ConditionalFormatter;
#[cfg(feature = "regex-formatters")]
pub use ismatch::{IsMatchFormatter, RegexOptions};
pub use list::ListFormatter;
pub use null::NullFormatter;
#[cfg(feature = "plural")]
pub use plural::PluralLocalizationFormatter;
pub use substring::{SubStringFormatter, SubStringOutOfRangeBehavior};
pub use template::{RegisterError, TemplateFormatter};

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

/// The parts `format` splits into, or the formatting error .NET's
/// `Format.IndexOf` throws when the search for the separators runs into a
/// crossed literal ([`SplitError::Count`](crate::parsing::SplitError::Count)).
///
/// All three formatters split *before* they count the parts, and .NET never
/// reaches the count when the search throws, so this has to be propagated with
/// `?` rather than folded into "requires at least N parts". Like the count
/// errors, the exception is one .NET raises inside `TryEvaluateFormat` and the
/// evaluator only wraps on the way out, so the message carries no envelope.
///
/// Cutting a part out of the format is deferred to [`split_part`], because .NET
/// defers it too.
pub(crate) fn split_format(
    info: &FormattingInfo<'_>,
    format: &Format,
    separator: char,
) -> Result<Vec<SplitPiece>, Error> {
    format
        .split(separator)
        .map_err(|error| info.plain_error(&error.to_string(), info.error_position()))
}

/// The part a formatter picked, or the formatting error .NET's
/// `Format.Substring` throws while cutting that part out of a format whose
/// separators a crossed literal put out of bounds.
///
/// .NET's `SplitList` only holds the separator offsets and cuts each part when
/// it is indexed, so a part that is never picked never throws: for
/// `{0:choose(1|2|3):a|b|\u12}` only the argument 3 fails. Every access to a
/// part therefore goes through here rather than through the slice directly.
pub(crate) fn split_part<'a>(
    info: &FormattingInfo<'_>,
    part: &'a SplitPiece,
) -> Result<&'a Format, Error> {
    part.as_ref()
        .map_err(|error| info.plain_error(&error.to_string(), info.error_position()))
}
