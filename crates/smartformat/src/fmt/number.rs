//! .NET standard numeric format specifiers: `C`, `D`, `E`, `F`, `G`, `N`,
//! `P`, `X` (upper/lower, optional precision), plus the empty spec (general).
//!
//! Reference: .NET "Standard numeric format strings" documentation and
//! `System.Number` formatting behavior (banker's-rounding is NOT used —
//! .NET formatting rounds half away from zero).

use super::culture::CultureData;
use super::FormatSpecError;

/// The numeric types a template value can hold (from `Value::Int` /
/// `Value::Float`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Int(i64),
    Float(f64),
}

/// Formats `n` with a .NET *standard* numeric format spec (`""`, `"N2"`,
/// `"x8"`, …), producing byte-identical output to .NET's
/// `n.ToString(spec, culture)`.
///
/// Custom patterns (anything that isn't a single standard specifier letter
/// plus optional precision digits) return [`FormatSpecError::Unsupported`].
/// `D`/`X` applied to a `Float` return [`FormatSpecError::Invalid`], as in
/// .NET.
pub fn format_number(
    n: Number,
    spec: &str,
    culture: &CultureData,
) -> Result<String, FormatSpecError> {
    let _ = (n, spec, culture);
    todo!("milestone M1: implement standard numeric specifiers")
}
