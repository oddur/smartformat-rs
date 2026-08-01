/// How the formatter reacts to parsing or formatting errors.
///
/// Mirrors SmartFormat.NET's `ParseErrorAction` / `FormatErrorAction`.
/// The .NET default (`ThrowError`) maps to returning `Err`; the lenient
/// modes return `Ok` with the corresponding recovery behavior applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorAction {
    /// Fail the call with an error. (.NET: `ThrowError`, the default.)
    #[default]
    Error,
    /// Write the error message into the output. (.NET: `OutputErrorInResult`.)
    OutputErrorInResult,
    /// Skip the offending item silently. (.NET: `Ignore`.)
    Ignore,
    /// Leave the offending tokens verbatim in the output. (.NET: `MaintainTokens`.)
    MaintainTokens,
}

/// Formatter-wide settings, mirroring SmartFormat.NET's `SmartSettings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartSettings {
    pub parse_error_action: ErrorAction,
    pub format_error_action: ErrorAction,
    pub case_sensitive: CaseSensitivity,
    /// `string.Format`-compatible mode: braces are escaped by doubling them,
    /// formatter names are not parsed, and only
    /// [`DefaultFormatter`](crate::formatter::DefaultFormatter) is ever
    /// invoked (.NET `SmartSettings.StringFormatCompatibility`).
    ///
    /// [`SmartFormatter::new`](crate::SmartFormatter::new) passes this on to
    /// the parser; [`SmartFormatter::with_parser_settings`](crate::SmartFormatter::with_parser_settings)
    /// takes it from the parser settings instead, like the error action.
    pub string_format_compatibility: bool,
    /// The moment "now" means for time-relative formatting (`TimeFormatter`
    /// on a `DateTime`, date conditions in `ConditionalFormatter`).
    ///
    /// `None` reads the system's local time when a template needs it, which is
    /// what .NET's `SystemTime.Now` does by default; `Some` pins it, like
    /// .NET's `SystemTime.SetDateTimeNow` (used by every golden so output is
    /// deterministic).
    #[cfg(feature = "time")]
    pub now: Option<jiff::civil::DateTime>,
    /// The character alignment pads with (.NET
    /// `FormatterSettings.AlignmentFillCharacter`).
    pub alignment_fill_character: char,
}

// The .NET defaults; `#[derive(Default)]` would make the fill character NUL.
impl Default for SmartSettings {
    fn default() -> Self {
        Self {
            parse_error_action: ErrorAction::default(),
            format_error_action: ErrorAction::default(),
            case_sensitive: CaseSensitivity::default(),
            string_format_compatibility: false,
            #[cfg(feature = "time")]
            now: None,
            alignment_fill_character: ' ',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseSensitivity {
    #[default]
    CaseSensitive,
    CaseInsensitive,
}

impl CaseSensitivity {
    /// Compares two names the way .NET does for the corresponding
    /// `StringComparison`: `Ordinal` or `OrdinalIgnoreCase`.
    pub fn eq(self, a: &str, b: &str) -> bool {
        match self {
            CaseSensitivity::CaseSensitive => a == b,
            CaseSensitivity::CaseInsensitive => eq_ordinal_ignore_case(a, b),
        }
    }
}

/// .NET `StringComparison.OrdinalIgnoreCase`, which uppercases each UTF-16
/// code unit with the invariant *simple* (one-to-one) mapping, so `ä` matches
/// `Ä` but `ß` never matches `SS`.
fn eq_ordinal_ignore_case(a: &str, b: &str) -> bool {
    let mut left = a.chars();
    let mut right = b.chars();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if simple_upper(x) == simple_upper(y) => continue,
            _ => return false,
        }
    }
}

/// The invariant simple uppercase mapping: a character whose full mapping
/// grows (`ß` → `SS`) is left alone, as in .NET.
fn simple_upper(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinal_ignore_case_folds_non_ascii() {
        let insensitive = CaseSensitivity::CaseInsensitive;
        assert!(insensitive.eq("Ä", "ä"));
        assert!(insensitive.eq("STRASSE", "strasse"));
        assert!(insensitive.eq("ǅ", "ǆ"));
        // .NET's one-to-one mapping never equates 'ß' with "ss".
        assert!(!insensitive.eq("straße", "strasse"));
        assert!(!insensitive.eq("ab", "abc"));
        assert!(insensitive.eq("", ""));
    }

    #[test]
    fn case_sensitive_is_ordinal() {
        let sensitive = CaseSensitivity::CaseSensitive;
        assert!(sensitive.eq("Name", "Name"));
        assert!(!sensitive.eq("Name", "name"));
    }
}
