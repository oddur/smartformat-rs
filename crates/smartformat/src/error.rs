use std::fmt;

/// Errors produced while parsing a template or formatting values into it.
#[derive(Debug)]
pub enum Error {
    /// The template could not be parsed. Carries the position and message
    /// of each syntax error found.
    Parse { errors: Vec<ParseError> },
    /// A placeholder could not be evaluated against the provided values.
    Format { message: String, position: usize },
    /// An escape sequence that resolves to nothing was reached while rendering.
    ///
    /// This is .NET's `ArgumentException` from `LiteralText.AsSpan()` and
    /// `Placeholder.FormatterOptions`, which resolve escape sequences lazily:
    /// the sequence is only rejected when the literal is written or the
    /// options are read, never when the template is parsed. Kept apart from
    /// [`Error::Parse`] because it is raised while formatting, and from
    /// [`Error::Format`] because .NET raises it outside the error handling of
    /// the evaluator — a literal of the top-level format fails the call
    /// whatever [`ErrorAction`](crate::ErrorAction) is set. When it does reach
    /// that error handling — the literal is inside a placeholder's format —
    /// it turns into an [`Error::Format`], as .NET wraps it in a
    /// `FormattingException`.
    Escape { message: String, position: usize },
    /// The format specifier is valid .NET but outside the supported subset,
    /// such as a custom numeric or date pattern. Kept apart from
    /// [`Error::Format`] so compatibility gaps are loud rather than silently
    /// wrong (see `DESIGN.md`).
    UnsupportedSpec {
        /// The offending specifier, as written in the template.
        spec: String,
        message: String,
        position: usize,
    },
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse { errors } => {
                write!(f, "template parse error")?;
                for e in errors {
                    write!(f, "; at {}: {}", e.position, e.message)?;
                }
                Ok(())
            }
            Error::Format { message, position } | Error::Escape { message, position } => {
                write!(f, "formatting error at {position}: {message}")
            }
            Error::UnsupportedSpec {
                message, position, ..
            } => {
                write!(f, "formatting error at {position}: {message}")
            }
        }
    }
}

impl std::error::Error for Error {}
