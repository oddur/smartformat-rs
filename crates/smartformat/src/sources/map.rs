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
    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        if let Some(null) = info.nullable_result() {
            return Some(null);
        }

        let Value::Map(map) = info.current else {
            return None;
        };

        let found = match info.settings.case_sensitive {
            CaseSensitivity::CaseSensitive => map.get(info.text()),
            comparison => map
                .iter()
                .find(|(key, _)| comparison.eq(key, info.text()))
                .map(|(_, value)| value),
        };

        if let Some(value) = found {
            return Some(Cow::Borrowed(value));
        }

        // A missing key is not an error when the selector is null-conditional.
        if info.has_nullable_operator() {
            return Some(Cow::Owned(Value::Null));
        }

        None
    }
}
