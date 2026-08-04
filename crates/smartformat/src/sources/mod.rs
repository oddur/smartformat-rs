//! Selector evaluation over [`Value`]s.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/Core/Extensions/Source.cs`,
//! `ISource.cs` and the source extensions in `src/SmartFormat/Extensions/`
//! (`DefaultSource.cs`, `DictionarySource.cs`, `StringSource.cs`, and the
//! selector half of `ListFormatter.cs`).
//!
//! A [`Source`] answers one question: "given this value and this selector,
//! what is the next value?". The [`SourceRegistry`] asks the registered
//! sources in order and takes the first answer, exactly like .NET's
//! `Registry.InvokeSourceExtensions`.

use std::borrow::Cow;

use crate::parsing::chars::NULLABLE_OPERATOR;
use crate::parsing::{Placeholder, Selector};
use crate::registry;
use crate::settings::SmartSettings;
use crate::value::Value;

mod default_source;
mod list;
mod map;
mod string;
pub mod variables;

pub use default_source::DefaultSource;
pub use list::ListSource;
pub use map::MapSource;
pub use string::StringSource;

/// Everything a [`Source`] needs to evaluate a single selector, mirroring
/// .NET `ISelectorInfo`.
#[derive(Debug, Clone, Copy)]
pub struct SelectorInfo<'a> {
    /// The value the selector is evaluated against.
    pub current: &'a Value,
    /// The selector being evaluated.
    pub selector: &'a Selector,
    /// The placeholder the selector belongs to.
    pub placeholder: &'a Placeholder,
    /// The arguments the format call was made with, for positional selectors
    /// (.NET `FormatDetails.OriginalArgs`).
    pub args: &'a [Value],
    pub settings: &'a SmartSettings,
    /// The index of the list item being formatted, or
    /// [`NO_COLLECTION_INDEX`](crate::formatter::NO_COLLECTION_INDEX) outside
    /// any list (.NET `ListFormatter.CollectionIndex`).
    ///
    /// [`ListSource`] answers the `{Index}` selector with it. .NET reads a
    /// `static` there, which is how a source — created once and shared by every
    /// call — can see what the `list` *formatter* is doing; here the state
    /// belongs to the format call and is handed to the source, so two calls at
    /// once cannot disturb each other.
    pub collection_index: i32,
}

impl<'a> SelectorInfo<'a> {
    /// The selector text without its operator.
    pub fn text(&self) -> &'a str {
        &self.selector.text
    }

    /// The operator preceding the selector: `""`, `"."`, `"?."`, `"["`, …
    pub fn operator(&self) -> &'a str {
        &self.selector.operator
    }

    /// The position of the selector in its placeholder, starting at 0.
    pub fn index(&self) -> usize {
        self.selector.index
    }

    /// Compares a name to the selector text, honoring
    /// [`SmartSettings::case_sensitive`].
    pub fn selector_is(&self, name: &str) -> bool {
        self.settings.case_sensitive.eq(self.text(), name)
    }

    /// Whether *any* selector of the placeholder carries the nullable
    /// operator (.NET `Source.HasNullableOperator`).
    ///
    /// The whole chain counts, not just the selectors up to this one, so
    /// `{City.Length?.Nope}` short-circuits to empty when `City` is null even
    /// though the `?.` sits on a later selector. That is what SmartFormat.NET
    /// 3.6.1 — the version the goldens are generated with — does; later .NET
    /// revisions restrict the scan to selectors up to the current one.
    pub fn has_nullable_operator(&self) -> bool {
        self.placeholder
            .selectors
            .iter()
            .any(|s| s.operator.starts_with(NULLABLE_OPERATOR) && s.operator.len() > 1)
    }

    /// `Some(null)` when the selector chain is null-conditional and the current
    /// value is null, which short-circuits the chain instead of failing
    /// (.NET `Source.TrySetResultForNullableOperator`).
    pub fn nullable_result(&self) -> Option<Cow<'a, Value>> {
        if self.has_nullable_operator() && matches!(self.current, Value::Null) {
            Some(Cow::Owned(Value::Null))
        } else {
            None
        }
    }
}

/// Resolves one selector against the current value, mirroring .NET `ISource`.
///
/// Returning `None` means "not handled" and lets the next source try; a
/// handled selector that legitimately has no value returns
/// `Some(Cow::Owned(Value::Null))`.
pub trait Source: Send + Sync {
    /// The value the selector resolves to, borrowed wherever it already
    /// exists.
    ///
    /// The source is borrowed for the same `'a` as the rest of the call, which
    /// a registered source always outlives — it is owned by the
    /// [`SmartFormatter`](crate::SmartFormatter) doing the formatting — so a
    /// source that *stores* values may hand out `Cow::Borrowed` into its own
    /// storage rather than copying, as
    /// [`PersistentVariablesSource`](variables::PersistentVariablesSource)
    /// does with its groups. `Cow::Owned` is for a value that has to be built,
    /// such as [`StringSource`]'s `ToUpper`.
    ///
    /// An implementation written `fn try_evaluate_selector<'a>(&self, …)` — the
    /// signature before the lifetimes were tied together — still satisfies
    /// this, since it promises more than is asked of it. Only a *caller* that
    /// held a source shorter-lived than the [`SelectorInfo`] has to change.
    fn try_evaluate_selector<'a>(&'a self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>>;

    /// The rank .NET's `WellKnownExtensionTypes.Sources` gives this source,
    /// which is where [`SourceRegistry::add`] slots it, or `None` for a source
    /// .NET's table does not hold — which is appended, exactly as there.
    ///
    /// .NET keys that table on the CLR type. A [`Source`] has no name to key on
    /// the way a [`Formatter`](crate::formatter::Formatter) does, so each
    /// source states its own rank instead; a custom source leaves this alone
    /// and is added last.
    fn well_known_rank(&self) -> Option<u32> {
        None
    }
}

/// The ranks .NET's `WellKnownExtensionTypes.Sources` lists for the sources
/// this port has. The names in the comments are .NET's types.
pub mod source_ranks {
    /// .NET `GlobalVariablesSource`.
    pub const GLOBAL_VARIABLES: u32 = 1000;
    /// .NET `PersistentVariablesSource`.
    pub const PERSISTENT_VARIABLES: u32 = 2000;
    /// .NET `StringSource`.
    pub const STRING: u32 = 3000;
    /// .NET `ListFormatter`, which is a source as well as a formatter.
    pub const LIST: u32 = 4000;
    /// .NET `DictionarySource`.
    pub const MAP: u32 = 5000;
    /// .NET `DefaultSource`.
    pub const DEFAULT: u32 = 12000;
}

/// The ordered list of [`Source`]s a [`SmartFormatter`](crate::SmartFormatter)
/// consults, mirroring .NET's source extension registry.
pub struct SourceRegistry {
    sources: Vec<Box<dyn Source>>,
}

impl SourceRegistry {
    /// An empty registry. Every selector fails until a source is added.
    pub fn empty() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    /// The M1 sources, in the order of .NET `WellKnownExtensionTypes.Sources`:
    /// strings, lists, maps, positional arguments.
    pub fn new() -> Self {
        Self {
            sources: vec![
                Box::new(StringSource),
                Box::new(ListSource),
                Box::new(MapSource),
                Box::new(DefaultSource),
            ],
        }
    }

    /// Appends a source, which is consulted after all existing ones.
    pub fn push(&mut self, source: Box<dyn Source>) {
        self.sources.push(source);
    }

    /// Adds a source at the position .NET's `WellKnownExtensionTypes.Sources`
    /// gives it (`Registry.AddExtensions`), so that a source added to the
    /// default registry ends up where a .NET registry holding it would have
    /// put it: a variables source ahead of [`StringSource`], whatever else is
    /// registered.
    ///
    /// A source .NET does not know — one whose
    /// [`well_known_rank`](Source::well_known_rank) is `None` — is appended,
    /// exactly as there, which puts it *after* [`DefaultSource`], where it
    /// answers only the selectors nothing else did. A custom source that has
    /// to be consulted earlier wants [`insert`](Self::insert) instead.
    pub fn add(&mut self, source: Box<dyn Source>) {
        let index = self.index_to_insert(source.well_known_rank());
        self.sources.insert(index, source);
    }

    /// Where a source of that rank belongs, a port of
    /// `WellKnownExtensionTypes.GetIndexToInsert`: after the last source ranked
    /// at or before it, or at the end when either is not well known.
    fn index_to_insert(&self, rank: Option<u32>) -> usize {
        registry::index_to_insert(
            self.sources.iter().map(|source| source.well_known_rank()),
            rank,
        )
    }

    /// Inserts a source at `index`, which is consulted before the sources
    /// currently at and after that position.
    pub fn insert(&mut self, index: usize, source: Box<dyn Source>) {
        self.sources.insert(index, source);
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// The value of the first source that handles the selector, or `None` if
    /// none does.
    pub fn evaluate<'a>(&'a self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        self.sources
            .iter()
            .find_map(|source| source.try_evaluate_selector(info))
    }
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SourceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceRegistry")
            .field("sources", &self.sources.len())
            .finish()
    }
}
