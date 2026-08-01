//! Ported from SmartFormat.NET `src/SmartFormat/Extensions/DictionarySource.cs`.

use std::borrow::Cow;

use super::{SelectorInfo, Source};
use crate::settings::CaseSensitivity;
use crate::value::Value;

/// Resolves a selector as a key of a [`Value::Map`], honoring
/// [`SmartSettings::case_sensitive`](crate::SmartSettings::case_sensitive).
#[derive(Debug, Default, Clone, Copy)]
pub struct MapSource;

impl Source for MapSource {
    fn well_known_rank(&self) -> Option<u32> {
        Some(super::source_ranks::MAP)
    }

    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        if let Some(null) = info.nullable_result() {
            return Some(null);
        }

        let Value::Map(map) = info.current else {
            return None;
        };

        // An exactly spelled key wins under either setting: it is the cheap
        // lookup, and the answer a reader expects when a map holds several
        // case-variants of one key.
        if let Some(value) = map.get(info.text()) {
            return Some(Cow::Borrowed(value));
        }

        if info.settings.case_sensitive == CaseSensitivity::CaseInsensitive {
            let found = map
                .iter()
                .find(|(key, _)| CaseSensitivity::CaseInsensitive.eq(key, info.text()))
                .map(|(_, value)| value);
            if let Some(value) = found {
                return Some(Cow::Borrowed(value));
            }
        }

        // A missing key is *not* handled here, even when the selector chain is
        // null-conditional: .NET only null-guards a null current value, so
        // `{Person?.Missing}` on a non-null map is a formatting error.
        None
    }
}
