//! Ported from the selector half of SmartFormat.NET
//! `src/SmartFormat/Extensions/ListFormatter.cs`. The formatter half (`list`)
//! lands with milestone M3.

use std::borrow::Cow;

use super::{SelectorInfo, Source};
use crate::value::Value;

/// Resolves a numeric selector against a [`Value::List`], as in
/// `{People[2].Name}` or `{Person.Nicknames.0}`.
///
/// A leading number with no operator is an argument index handled by
/// [`DefaultSource`](super::DefaultSource), so this source ignores it — the
/// same "is absolute" check .NET makes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ListSource;

impl Source for ListSource {
    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        let Value::List(items) = info.current else {
            return None;
        };

        let is_absolute = info.index() == 0 && info.operator().is_empty();
        if is_absolute {
            return None;
        }

        let index: usize = info.text().parse().ok()?;
        items.get(index).map(Cow::Borrowed)
    }
}
