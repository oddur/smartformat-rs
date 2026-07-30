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
#[derive(Debug, Clone, Default)]
pub struct SmartSettings {
    pub parse_error_action: ErrorAction,
    pub format_error_action: ErrorAction,
    pub case_sensitive: CaseSensitivity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseSensitivity {
    #[default]
    CaseSensitive,
    CaseInsensitive,
}
