//! Ported from the selector half of SmartFormat.NET
//! `src/SmartFormat/Extensions/ListFormatter.cs` — the class is an `ISource` as
//! well as an `IFormatter`, and the formatter half is
//! [`ListFormatter`](crate::extensions::list::ListFormatter).

use std::borrow::Cow;

use super::{SelectorInfo, Source};
use crate::value::Value;

/// The selector that answers the index of the list item being formatted,
/// matched ignoring case as .NET's `OrdinalIgnoreCase` does — whatever
/// [`SmartSettings::case_sensitive`](crate::SmartSettings::case_sensitive) says.
const INDEX: &str = "Index";

/// Resolves a numeric selector against a [`Value::List`], as in
/// `{People[2].Name}` or `{Person.Nicknames.0}`, and the `{Index}` selector
/// that goes with the [`list`](crate::extensions::list::ListFormatter)
/// formatter.
///
/// `{Index}` is three things at once, all of them .NET's:
///
/// * the index of the item being formatted, inside a `list` format:
///   `{0:list:{} = {Index}|, }`;
/// * the same index applied to *another* list, which is how two lists are
///   iterated in step: `{0:list:{} = {1[Index]}|, }`;
/// * `-1` outside any list, so `{Index}` on its own renders `-1` rather than
///   failing.
///
/// A leading number with no operator is an argument index handled by
/// [`DefaultSource`](super::DefaultSource), so this source ignores it — the
/// same "is absolute" check .NET makes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ListSource;

impl Source for ListSource {
    fn well_known_rank(&self) -> Option<u32> {
        Some(super::source_ranks::LIST)
    }

    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        // .NET asks `current is IEnumerable`, which a string and a dictionary
        // answer as well as a list: `{Index}` renders -1 for all three (probed
        // against 3.6.1). Only the *formatter* half excludes strings.
        let selector_is_index = matches!(
            info.current,
            Value::List(_) | Value::String(_) | Value::Map(_)
        ) && info.text().eq_ignore_ascii_case(INDEX);

        // As the first selector of a placeholder, `Index` is the ambient index
        // itself — `{Index}` — rather than an index into the current value.
        if selector_is_index && info.index() == 0 {
            return Some(Cow::Owned(Value::Int(info.collection_index.into())));
        }

        let Value::List(items) = info.current else {
            return None;
        };

        // A number as the first selector with no operator is an argument index,
        // not an index into this list.
        let is_absolute = info.index() == 0 && info.operator().is_empty();
        if !is_absolute {
            if let Ok(index) = info.text().parse::<usize>() {
                // An index past the end is *not* handled here, as in .NET,
                // which leaves such a selector to the sources after this one
                // and then to the enclosing scopes.
                if let Some(item) = items.get(index) {
                    return Some(Cow::Borrowed(item));
                }
            }
        }

        // Two lists iterated in step: `{List1:list:{List2[Index]}|, }`. An
        // index out of range — a second list shorter than the first, or no
        // iteration going on at all — is left to the enclosing scopes, exactly
        // as .NET leaves it.
        if selector_is_index {
            let index = usize::try_from(info.collection_index).ok()?;
            return items.get(index).map(Cow::Borrowed);
        }

        None
    }
}
