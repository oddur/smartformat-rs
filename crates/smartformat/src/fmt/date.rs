//! .NET standard date/time format specifiers: `d D f F g G m M o O r R s
//! t T u y Y` (and the empty spec = `G`), rendered from a
//! [`jiff::civil::DateTime`].
//!
//! Each standard specifier expands to the culture's corresponding pattern
//! (`d` → `short_date_pattern`, …) which is then rendered by an internal
//! .NET-custom-pattern interpreter. That interpreter exists only to back the
//! standard specifiers; user-supplied custom patterns are still rejected
//! with [`FormatSpecError::Unsupported`] by [`format_datetime`].
//!
//! `U` (universal full) requires timezone conversion and is Unsupported in
//! M1; `K`-related offsets render as empty, matching an unspecified-kind
//! .NET `DateTime`.

use super::culture::CultureData;
use super::FormatSpecError;

/// Formats `dt` with a .NET *standard* date/time format spec, producing
/// byte-identical output to .NET's `dt.ToString(spec, culture)` for a
/// `DateTime` of unspecified kind.
pub fn format_datetime(
    dt: &jiff::civil::DateTime,
    spec: &str,
    culture: &CultureData,
) -> Result<String, FormatSpecError> {
    let _ = (dt, spec, culture);
    todo!("milestone M1: implement standard date/time specifiers")
}
