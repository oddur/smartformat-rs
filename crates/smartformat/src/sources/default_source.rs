//! Ported from SmartFormat.NET `src/SmartFormat/Extensions/DefaultSource.cs`.

use std::borrow::Cow;

use super::{SelectorInfo, Source};
use crate::value::Value;

/// Resolves `{0}`, `{1}`, … against the arguments the format call was made
/// with, just like `string.Format`.
///
/// As in .NET, the index must be the first selector, must carry no operator,
/// and must be in range.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultSource;

impl Source for DefaultSource {
    fn well_known_rank(&self) -> Option<u32> {
        Some(super::source_ranks::DEFAULT)
    }

    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        let index: usize = info.text().parse().ok()?;
        if info.index() == 0 && info.operator().is_empty() && index < info.args.len() {
            return Some(Cow::Borrowed(&info.args[index]));
        }
        None
    }
}
