//! The messages .NET's own exceptions carry, where more than one place in the
//! port has to reproduce one.
//!
//! Every string here is BCL text quoted verbatim, and every one of them is
//! observable: an exception the evaluator catches reaches the output bare under
//! [`ErrorAction::OutputErrorInResult`](crate::ErrorAction::OutputErrorInResult),
//! and the goldens pin it against SmartFormat.NET. A reworded copy would be a
//! rendering difference, so the text lives once and is referenced.

/// `ArgumentOutOfRangeException`'s message, with the parameter name .NET's
/// `nameof(…)` appended when the throwing API passed one.
macro_rules! out_of_range {
    () => {
        "Specified argument was out of the range of valid values."
    };
    ($parameter:literal) => {
        concat!(out_of_range!(), " (Parameter '", $parameter, "')")
    };
}

/// `OverflowException` from `int.Parse` and from an `(int)` cast of a value
/// too large to hold (`System.SR.Overflow_Int32`).
pub(crate) const INT32_OVERFLOW: &str = "Value was either too large or too small for an Int32.";

/// `ArgumentOutOfRangeException` thrown with no parameter name, which is what
/// `ReadOnlySpan<char>.Slice` does (`System.SR.Arg_ArgumentOutOfRangeException`).
pub(crate) const OUT_OF_RANGE: &str = out_of_range!();

/// [`OUT_OF_RANGE`] for a `start` argument (.NET `Format.Substring`).
pub(crate) const OUT_OF_RANGE_START: &str = out_of_range!("start");

/// [`OUT_OF_RANGE`] for a `length` argument (.NET `Format.Substring`).
pub(crate) const OUT_OF_RANGE_LENGTH: &str = out_of_range!("length");

/// [`OUT_OF_RANGE`] for an `index` argument, which .NET's `ConditionalFormatter`
/// reaches by indexing one past the last parameter.
pub(crate) const OUT_OF_RANGE_INDEX: &str = out_of_range!("index");

/// `FormatException` from `int.Parse` and `decimal.Parse`
/// (`System.SR.Format_InvalidStringWithValue`), which quotes the input as it
/// was written, whitespace included.
pub(crate) fn not_in_a_correct_format(text: &str) -> String {
    format!("The input string '{text}' was not in a correct format.")
}
