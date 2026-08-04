//! What the two extension registries — [`SourceRegistry`] and
//! [`FormatterRegistry`] — share: where .NET slots an extension it knows.
//!
//! [`SourceRegistry`]: crate::sources::SourceRegistry
//! [`FormatterRegistry`]: crate::formatter::FormatterRegistry

/// Where an extension of rank `rank` belongs among extensions of the given
/// ranks, a port of `WellKnownExtensionTypes.GetIndexToInsert`: after the last
/// extension ranked at or before it, or at the end when either that extension
/// or every registered one is unknown to .NET.
///
/// `ranks` is the rank of each registered extension, in registry order, `None`
/// for one .NET's table does not hold.
pub(crate) fn index_to_insert<I>(ranks: I, rank: Option<u32>) -> usize
where
    I: IntoIterator<Item = Option<u32>>,
    I::IntoIter: DoubleEndedIterator + ExactSizeIterator,
{
    let ranks = ranks.into_iter();
    let Some(rank) = rank else {
        return ranks.len();
    };
    for (index, other) in ranks.enumerate().rev() {
        if matches!(other, Some(other) if other <= rank) {
            return index + 1;
        }
    }
    0
}
