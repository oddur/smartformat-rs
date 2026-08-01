//! Ported from SmartFormat.NET `src/SmartFormat/Extensions/PersistentVariablesSource.cs`,
//! `GlobalVariablesSource.cs` and the types in
//! `src/SmartFormat/Extensions/PersistentVariables/`.
//!
//! Variables that a template can name without them being passed as arguments:
//! register a named group on the source, and `{groupName.variableName}`
//! resolves against it.
//!
//! ```
//! use smartformat::sources::variables::{self, PersistentVariablesSource};
//! use smartformat::{SmartFormatter, Value};
//!
//! let mut source = PersistentVariablesSource::new();
//! source.add(
//!     "global",
//!     variables::group([
//!         ("user", Value::from("Joe")),
//!         ("theme", Value::from("dark")),
//!     ]),
//! );
//!
//! let mut smart = SmartFormatter::default();
//! // .NET ranks both variable sources ahead of every other source
//! // (`WellKnownExtensionTypes.Sources`: global 1000, persistent 2000, string
//! // 3000, …), which in the default registry is index 0.
//! smart.sources_mut().insert(0, Box::new(source));
//!
//! // No arguments carry the variables.
//! assert_eq!(
//!     smart.format("{global.user} likes {global.theme}", &Value::Null).unwrap(),
//!     "Joe likes dark"
//! );
//! ```
//!
//! # What a group is here
//!
//! .NET has a `VariablesGroup : IDictionary<string, IVariable>` and boxes every
//! value into an `IVariable` (`IntVariable`, `StringVariable`,
//! `ObjectVariable`, …). Those wrappers exist for two reasons that a port does
//! not have: `object` needs a marker interface for the source to recognise a
//! group among the arguments, and a group has to be storable in the same
//! dictionary as a scalar. A [`Value`] is already that union, so a group is
//! just a map of names to [`Value`]s — [`VariablesGroup`] is a type alias for
//! it — a nested group is a [`Value::Map`] like any other map, and `IVariable`
//! / `Variable<T>` / `NameVariablePair` have no counterpart. Every formatter
//! therefore works on a variable exactly as it works on an argument.
//!
//! # Which source to use
//!
//! [`PersistentVariablesSource`] owns its groups: fill it, then register it,
//! after which the registry hands out no way back to it.
//! [`GlobalVariablesSource`] is the same source over shared storage, so a
//! clone of the handle kept by the caller can add and remove groups after the
//! source is registered, and several [`SmartFormatter`]s can share one set of
//! variables.
//!
//! [`SmartFormatter`]: crate::SmartFormatter

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::{SelectorInfo, Source};
use crate::value::Value;

/// A named collection of variables — .NET's `VariablesGroup`.
///
/// It is the very map a [`Value::Map`] holds, so a group can be nested inside
/// another group, handed to a formatter, or passed as an argument without
/// conversion.
pub type VariablesGroup = BTreeMap<String, Value>;

/// Builds a [`VariablesGroup`] from name/value pairs, the way .NET's
/// collection initialiser does.
///
/// ```
/// use smartformat::sources::variables;
/// use smartformat::Value;
///
/// let group = variables::group([
///     ("name", Value::from("Joe")),
///     ("nested", Value::Map(variables::group([("age", Value::from(42i64))]))),
/// ]);
/// assert_eq!(group["name"], Value::from("Joe"));
/// ```
pub fn group<K, V>(variables: impl IntoIterator<Item = (K, V)>) -> VariablesGroup
where
    K: Into<String>,
    V: Into<Value>,
{
    variables
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect()
}

/// Resolves a selector against the groups of `groups`, a port of
/// `PersistentVariablesSource.TryEvaluateSelector`.
///
/// The three steps are .NET's, in .NET's order:
///
/// 1. a null current value under a null-conditional chain short-circuits, as
///    in every other source;
/// 2. a *group* as the current value answers from its own variables, which is
///    what makes an argument override a registered group of the same name;
/// 3. otherwise the selector is looked up among the registered group names —
///    unconditionally, whatever the current value is, so a group named `Length`
///    answers `{0.Length}` on a string before [`StringSource`] gets to see it
///    (probed against 3.6.1).
///
/// Both lookups are exact, ordinal comparisons: .NET reaches for a plain
/// `Dictionary<string, …>` with the default comparer, and never consults
/// [`SmartSettings::case_sensitive`]. With `CaseSensitivity::CaseInsensitive`
/// set, 3.6.1 still fails `{GLOBAL.theVariable}`.
///
/// Step 2 is where the port cannot be exact, because a group is a
/// [`Value::Map`] and so is every other map. .NET tells a `VariablesGroup`
/// from a `Dictionary` by its interface, and gives only the former the right
/// to answer before the group names; here both do, and a map argument
/// therefore shadows a registered group of the same name. The same one map
/// type also leaves a variable name to [`MapSource`](super::MapSource) after
/// this source declines it, which under case-insensitive settings finds what
/// .NET would not. Both are in DESIGN.md, "Known divergences".
///
/// [`StringSource`]: super::StringSource
/// [`SmartSettings::case_sensitive`]: crate::SmartSettings::case_sensitive
fn evaluate<'a>(groups: &VariablesGroup, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
    if let Some(null) = info.nullable_result() {
        return Some(null);
    }

    if let Value::Map(current) = info.current {
        if let Some(value) = current.get(info.text()) {
            return Some(Cow::Borrowed(value));
        }
    }

    // A registered group is cloned rather than borrowed: what a source returns
    // is tied to the format call, not to the source, so there is no lifetime
    // to lend it. Only the group itself is copied — the selector after it
    // reads out of the copy without copying again.
    groups
        .get(info.text())
        .map(|group| Cow::Owned(group.clone()))
}

/// Variables that templates can name without them being passed as arguments,
/// ported from .NET `PersistentVariablesSource`.
///
/// Groups are added by name and reached as `{groupName.variableName}`; a
/// nested group extends the chain, `{groupName.inner.variableName}`.
///
/// This source owns its groups, so it has to be filled before it is
/// registered. [`GlobalVariablesSource`] is the same source over storage that
/// stays reachable afterwards.
///
/// ```
/// use smartformat::sources::variables::{self, PersistentVariablesSource};
/// use smartformat::{SmartFormatter, Value};
///
/// let source = PersistentVariablesSource::from_iter([(
///     "app",
///     variables::group([("name", Value::from("Acme"))]),
/// )]);
///
/// let mut smart = SmartFormatter::default();
/// smart.sources_mut().insert(0, Box::new(source));
/// assert_eq!(smart.format("{app.name}", &Value::Null).unwrap(), "Acme");
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersistentVariablesSource {
    groups: VariablesGroup,
}

impl PersistentVariablesSource {
    /// A source with no groups.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a group under `name`, returning the group it replaced, if any.
    ///
    /// .NET's `Add` is the indexer setter, so it replaces silently too. Unlike
    /// .NET it accepts an empty name instead of throwing: no selector can be
    /// empty, so such a group is merely unreachable.
    ///
    /// The group is a [`Value`] rather than a `VariablesGroup` because .NET's
    /// requirement that it be a group is worth nothing here — a scalar group
    /// simply renders as `{name}` and fails any selector below it.
    pub fn add(&mut self, name: impl Into<String>, group: impl Into<Value>) -> Option<Value> {
        self.groups.insert(name.into(), group.into())
    }

    /// The group registered under `name`.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.groups.get(name)
    }

    /// The group registered under `name`, to change in place.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.groups.get_mut(name)
    }

    /// Removes the group registered under `name` and returns it.
    pub fn remove(&mut self, name: &str) -> Option<Value> {
        self.groups.remove(name)
    }

    /// Whether a group is registered under `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.groups.contains_key(name)
    }

    /// Removes every group.
    pub fn clear(&mut self) {
        self.groups.clear();
    }

    /// The number of registered groups.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// The registered groups, by name.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&String, &Value)> {
        self.groups.iter()
    }

    /// The names of the registered groups.
    pub fn names(&self) -> impl ExactSizeIterator<Item = &String> {
        self.groups.keys()
    }
}

impl<K, V> FromIterator<(K, V)> for PersistentVariablesSource
where
    K: Into<String>,
    V: Into<Value>,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(groups: I) -> Self {
        Self {
            groups: group(groups),
        }
    }
}

impl Source for PersistentVariablesSource {
    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        evaluate(&self.groups, info)
    }
}

/// [`PersistentVariablesSource`] over shared storage: a clone-able handle onto
/// one set of groups, ported from .NET `GlobalVariablesSource`.
///
/// .NET's version is `PersistentVariablesSource` over a `static` store reached
/// through `GlobalVariablesSource.Instance`, with `Reset()` to swap in a fresh
/// one. This port shares an `Arc<RwLock<…>>` instead of hiding a process-wide
/// singleton: clone the handle to share the variables, drop every clone to
/// reset them. Selector resolution is identical — only the sharing differs.
///
/// The handle is what makes variables changeable after registration, since a
/// registered source cannot be reached through the registry:
///
/// ```
/// use smartformat::sources::variables::{self, GlobalVariablesSource};
/// use smartformat::{SmartFormatter, Value};
///
/// let variables = GlobalVariablesSource::new();
/// let mut smart = SmartFormatter::default();
/// smart.sources_mut().insert(0, Box::new(variables.clone()));
///
/// variables.add("app", variables::group([("name", Value::from("Acme"))]));
/// assert_eq!(smart.format("{app.name}", &Value::Null).unwrap(), "Acme");
///
/// variables.add("app", variables::group([("name", Value::from("Globex"))]));
/// assert_eq!(smart.format("{app.name}", &Value::Null).unwrap(), "Globex");
/// ```
#[derive(Debug, Clone, Default)]
pub struct GlobalVariablesSource {
    groups: Arc<RwLock<VariablesGroup>>,
}

impl GlobalVariablesSource {
    /// A handle onto a new, empty set of groups.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a group under `name`, returning the group it replaced, if any.
    ///
    /// Every clone of the handle sees the change, including one already
    /// registered on a [`SmartFormatter`](crate::SmartFormatter).
    pub fn add(&self, name: impl Into<String>, group: impl Into<Value>) -> Option<Value> {
        self.write().insert(name.into(), group.into())
    }

    /// A copy of the group registered under `name`.
    ///
    /// The group is copied because it lives behind a lock; the owning
    /// [`PersistentVariablesSource::get`] lends it instead.
    pub fn get(&self, name: &str) -> Option<Value> {
        self.read().get(name).cloned()
    }

    /// Removes the group registered under `name` and returns it.
    pub fn remove(&self, name: &str) -> Option<Value> {
        self.write().remove(name)
    }

    /// Whether a group is registered under `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.read().contains_key(name)
    }

    /// Removes every group, which is what .NET's `GlobalVariablesSource.Reset`
    /// amounts to for handles that keep pointing at this storage.
    pub fn clear(&self) {
        self.write().clear();
    }

    /// The number of registered groups.
    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.read().is_empty()
    }

    /// A copy of every registered group, by name.
    pub fn snapshot(&self) -> VariablesGroup {
        self.read().clone()
    }

    // A panic while a group was being added leaves the map itself intact —
    // `BTreeMap` cannot be torn by an interrupted insert — so a poisoned lock
    // is taken as it is rather than made to poison every later format call.
    fn read(&self) -> RwLockReadGuard<'_, VariablesGroup> {
        self.groups.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, VariablesGroup> {
        self.groups.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<K, V> FromIterator<(K, V)> for GlobalVariablesSource
where
    K: Into<String>,
    V: Into<Value>,
{
    fn from_iter<I: IntoIterator<Item = (K, V)>>(groups: I) -> Self {
        Self {
            groups: Arc::new(RwLock::new(group(groups))),
        }
    }
}

impl From<PersistentVariablesSource> for GlobalVariablesSource {
    fn from(source: PersistentVariablesSource) -> Self {
        Self {
            groups: Arc::new(RwLock::new(source.groups)),
        }
    }
}

impl Source for GlobalVariablesSource {
    fn try_evaluate_selector<'a>(&self, info: SelectorInfo<'a>) -> Option<Cow<'a, Value>> {
        evaluate(&self.read(), info)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/PersistentVariableSourceTests.cs`,
    //! `GlobalVariableSourceTests.cs` and `VariablesGroupTests.cs`. The cases
    //! about .NET's dictionary surface that a `BTreeMap` answers by itself
    //! (`CopyTo`, the enumerators, `IsReadOnly`) are dropped; the parallel-load
    //! cases become the shared-handle cases, since thread safety here is not a
    //! setting.
    //!
    //! Every expectation was probed against the pinned SmartFormat.NET 3.6.1
    //! package, with the source registered by hand as .NET's tests do
    //! (`smart.InsertExtension(0, pvs)`).

    use super::*;
    use crate::settings::{CaseSensitivity, ErrorAction, SmartSettings};
    use crate::{Error, SmartFormatter};

    /// The .NET default of `FormatErrorAction.ThrowError`, so a selector that
    /// resolves to nothing surfaces as an error.
    fn settings() -> SmartSettings {
        SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        }
    }

    fn smart_with(source: impl Source + 'static) -> SmartFormatter {
        smart_with_settings(source, settings())
    }

    fn smart_with_settings(
        source: impl Source + 'static,
        settings: SmartSettings,
    ) -> SmartFormatter {
        let mut smart = SmartFormatter::new(settings);
        smart.sources_mut().insert(0, Box::new(source));
        smart
    }

    /// What .NET's `smart.Format("{global.x}")` formats against: an empty
    /// argument list, which becomes its own current value. It matters for the
    /// null-conditional cases — a [`Value::Null`] root would short-circuit
    /// them before the first selector is even tried.
    fn no_args() -> Value {
        Value::List(Vec::new())
    }

    /// The group of .NET's `Use_Globals_Without_Args_To_Formatter`: a nested
    /// group plus two variables of its own.
    fn top_group() -> VariablesGroup {
        group([
            (
                "group",
                Value::Map(group([("groupString", Value::from("groupStringValue"))])),
            ),
            ("topInteger", Value::from(12345i64)),
            ("topString", Value::from("topStringValue")),
        ])
    }

    fn source() -> PersistentVariablesSource {
        PersistentVariablesSource::from_iter([("global", top_group())])
    }

    // -- the dictionary surface ---------------------------------------------

    #[test]
    fn add_and_get_groups() {
        let mut source = PersistentVariablesSource::new();
        assert!(source.is_empty());

        source.add("global", group([("a", Value::from(1i64))]));
        source.add("secret", group([("b", Value::from(2i64))]));

        assert_eq!(source.len(), 2);
        assert!(source.contains("global"));
        assert!(source.contains("secret"));
        assert!(!source.contains("globalFalse"));
        assert_eq!(
            source.get("global"),
            Some(&Value::Map(group([("a", 1i64)])))
        );
        assert_eq!(source.get("nope"), None);
        assert_eq!(source.names().collect::<Vec<_>>(), vec!["global", "secret"]);
        assert_eq!(
            source.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            vec!["global", "secret"]
        );
    }

    /// .NET's `Add` is its indexer setter, so a repeated name replaces the
    /// group rather than throwing (probed: `Count` stays 1).
    #[test]
    fn adding_a_name_twice_replaces_the_group() {
        let mut source = PersistentVariablesSource::new();
        source.add("global", group([("a", Value::from(1i64))]));
        let replaced = source.add("global", group([("b", Value::from(2i64))]));

        assert_eq!(source.len(), 1);
        assert_eq!(replaced, Some(Value::Map(group([("a", 1i64)]))));
        assert_eq!(
            source.get("global"),
            Some(&Value::Map(group([("b", 2i64)])))
        );
    }

    #[test]
    fn remove_and_clear() {
        let mut source = source();
        assert_eq!(source.remove("nope"), None);
        assert!(source.remove("global").is_some());
        assert!(source.is_empty());

        let mut source = self::source();
        source.clear();
        assert!(source.is_empty());
    }

    #[test]
    fn a_group_can_be_changed_in_place() {
        let mut source = source();
        let Some(Value::Map(group)) = source.get_mut("global") else {
            panic!("the group should be a map");
        };
        group.insert("added".to_owned(), Value::from("later"));

        let smart = smart_with(source);
        assert_eq!(smart.format("{global.added}", &no_args()).unwrap(), "later");
    }

    /// .NET's `Shallow_Copy`: a copy holds the same groups.
    #[test]
    fn a_clone_holds_the_same_groups() {
        let source = source();
        let copy = source.clone();
        assert_eq!(copy, source);
        assert_eq!(copy.len(), source.len());
    }

    // -- formatting ----------------------------------------------------------

    /// .NET's `Use_Globals_Without_Args_To_Formatter`.
    #[test]
    fn variables_resolve_without_arguments() {
        let smart = smart_with(source());

        assert_eq!(
            smart
                .format("{global.group.groupString}", &no_args())
                .unwrap(),
            "groupStringValue"
        );
        assert_eq!(
            smart.format("{global.topInteger}", &no_args()).unwrap(),
            "12345"
        );
        assert_eq!(
            smart.format("{global.topString}", &no_args()).unwrap(),
            "topStringValue"
        );
    }

    /// The whole point of the source: a variable is a [`Value`], so the
    /// formatters work on one as they work on an argument. Probed in 3.6.1:
    /// `{v.i:N2}` is `1,234.00`, `{v.b:yes|no}` is `yes`, `{v.s.Length}` is 3,
    /// and the list formatter runs over a list variable.
    #[test]
    fn every_formatter_works_on_a_variable() {
        let smart = smart_with(PersistentVariablesSource::from_iter([(
            "v",
            group([
                ("i", Value::from(1234i64)),
                ("b", Value::from(true)),
                ("s", Value::from("str")),
                (
                    "list",
                    Value::List(vec![Value::from("a"), Value::from("b"), Value::from("c")]),
                ),
            ]),
        )]));

        assert_eq!(smart.format("{v.i:N2}", &no_args()).unwrap(), "1,234.00");
        assert_eq!(smart.format("{v.b:yes|no}", &no_args()).unwrap(), "yes");
        assert_eq!(smart.format("{v.s.Length}", &no_args()).unwrap(), "3");
        assert_eq!(
            smart
                .format("{v.list:list:{}|, |, and }", &no_args())
                .unwrap(),
            "a, b, and c"
        );
        assert_eq!(smart.format("{v.list.0}", &no_args()).unwrap(), "a");
    }

    #[cfg(feature = "time")]
    #[test]
    fn a_datetime_variable_keeps_its_format() {
        let smart = smart_with(PersistentVariablesSource::from_iter([(
            "global",
            group([(
                "groupDateTime",
                Value::DateTime(jiff::civil::date(2024, 12, 31).at(0, 0, 0, 0)),
            )]),
        )]));

        // .NET's case formats the date with a custom pattern,
        // `{…:'groupDateTime='yyyy-MM-dd}`; this port understands the standard
        // specifiers only, and which specs a date takes is the date
        // formatter's business, not this source's.
        assert_eq!(
            smart
                .format("{global.groupDateTime:d}", &no_args())
                .unwrap(),
            "12/31/2024"
        );
    }

    /// .NET's `NonExisting_GroupVariable_Should_Throw`: a group that exists but
    /// has no such variable is not handled here either, so the selector falls
    /// through every source and fails.
    #[test]
    fn a_missing_variable_of_an_existing_group_fails() {
        let smart = smart_with(source());

        let error = smart
            .format("{global.missing}", &no_args())
            .unwrap_err()
            .to_string();
        assert!(error.contains("missing"), "{error}");

        let error = smart
            .format("{missingGroup}", &no_args())
            .unwrap_err()
            .to_string();
        assert!(error.contains("missingGroup"), "{error}");
    }

    /// Probed in 3.6.1: `{global?.missing}` still fails. The null-conditional
    /// operator only short-circuits a *null* current value, and a group is not
    /// null.
    #[test]
    fn the_nullable_operator_does_not_excuse_a_missing_variable() {
        let smart = smart_with(source());
        assert!(smart.format("{global?.missing}", &no_args()).is_err());
    }

    /// .NET's `Should_Handle_null_for_Nullable`: a variable holding null under
    /// a null-conditional chain renders empty.
    #[test]
    fn a_null_variable_short_circuits_a_nullable_chain() {
        let smart = smart_with(PersistentVariablesSource::from_iter([(
            "Global",
            group([
                ("NullableVar", Value::Null),
                (
                    "NotNull",
                    Value::Map(group([("Anything", Value::from("not-null"))])),
                ),
            ]),
        )]));

        assert_eq!(
            smart
                .format("{Global.NullableVar?.Anything}", &no_args())
                .unwrap(),
            ""
        );
        assert_eq!(
            smart
                .format("{Global.NotNull?.Anything}", &no_args())
                .unwrap(),
            "not-null"
        );
        assert_eq!(
            smart.format("{Global.NullableVar}", &no_args()).unwrap(),
            ""
        );
    }

    /// .NET's `Format_Args_Should_Override_Persistent_Vars`: a group among the
    /// arguments answers first, and a group that has no such name lets the
    /// registered one answer.
    ///
    /// A map argument is what a `VariablesGroup` argument is in .NET, and also
    /// what a `Dictionary` argument is — which 3.6.1 keeps apart: probed
    /// there, a `Dictionary` holding `global` loses to a registered group of
    /// that name, because only an `IVariablesGroup` is consulted before the
    /// group names. With one map type there is one answer, and it is the
    /// documented one: the argument wins. See DESIGN.md, "Known divergences".
    #[test]
    fn arguments_override_registered_groups() {
        let smart = smart_with(PersistentVariablesSource::from_iter([(
            "global",
            group([("theVariable", Value::from("val-from-persistent-source"))]),
        )]));

        let argument = Value::Map(group([(
            "global",
            Value::Map(group([("theVariable", Value::from("val-from-argument"))])),
        )]));
        assert_eq!(
            smart.format("{global.theVariable}", &argument).unwrap(),
            "val-from-argument"
        );

        let unrelated = Value::Map(group([("somethingElse", Value::from("x"))]));
        assert_eq!(
            smart.format("{global.theVariable}", &unrelated).unwrap(),
            "val-from-persistent-source"
        );
        assert_eq!(
            smart.format("{global.theVariable}", &no_args()).unwrap(),
            "val-from-persistent-source"
        );
    }

    /// A group name is looked up whatever the current value is and wherever
    /// the selector sits, so it shadows the selectors of other sources. Probed
    /// in 3.6.1 with a group named `Length`: `{0.Length}` on a string yields
    /// the group, not 4, and `{0.Length.v}` reads out of the group.
    #[test]
    fn a_group_name_shadows_other_sources() {
        let smart = smart_with(PersistentVariablesSource::from_iter([(
            "Length",
            group([("v", Value::from(7i64))]),
        )]));

        let args = Value::List(vec![Value::from("abcd")]);
        assert_eq!(smart.format("{0.Length.v}", &args).unwrap(), "7");
        // `{0.Length}` resolves to the group itself; rendering a map is where
        // this port parts with .NET, which writes the CLR type name.
        assert!(matches!(
            smart.format("{0.Length}", &args),
            Err(Error::Format { .. })
        ));
    }

    /// Probed in 3.6.1: this source looks names up in a plain `Dictionary`
    /// with the default comparer, so a group name stays ordinal even when the
    /// settings say case-insensitive.
    #[test]
    fn group_names_are_ordinal_whatever_the_settings_say() {
        for case_sensitive in [
            CaseSensitivity::CaseSensitive,
            CaseSensitivity::CaseInsensitive,
        ] {
            let smart = smart_with_settings(
                source(),
                SmartSettings {
                    case_sensitive,
                    ..settings()
                },
            );

            assert_eq!(
                smart.format("{global.topString}", &no_args()).unwrap(),
                "topStringValue"
            );
            assert!(smart.format("{GLOBAL.topString}", &no_args()).is_err());
        }
    }

    /// A *variable* name is ordinal in this source too, but a group is a
    /// [`Value::Map`] here, so under case-insensitive settings
    /// [`MapSource`](super::MapSource) picks up what this source declined —
    /// which 3.6.1 does not do, because its `VariablesGroup` is an
    /// `IDictionary<string, IVariable>` that `DictionarySource` does not
    /// recognise. See DESIGN.md, "Known divergences".
    #[test]
    fn a_case_insensitive_setting_reaches_variable_names_too() {
        let sensitive = smart_with_settings(
            source(),
            SmartSettings {
                case_sensitive: CaseSensitivity::CaseSensitive,
                ..settings()
            },
        );
        assert!(sensitive.format("{global.TOPSTRING}", &no_args()).is_err());

        let insensitive = smart_with_settings(
            source(),
            SmartSettings {
                case_sensitive: CaseSensitivity::CaseInsensitive,
                ..settings()
            },
        );
        assert_eq!(
            insensitive
                .format("{global.TOPSTRING}", &no_args())
                .unwrap(),
            "topStringValue"
        );
    }

    // -- the shared handle ---------------------------------------------------

    /// .NET's `Global_Variables_In_Different_SmartFormatters`.
    #[test]
    fn one_handle_serves_several_formatters() {
        let variables = GlobalVariablesSource::from_iter([(
            "global",
            group([("theVariable", Value::from("val-from-global-source"))]),
        )]);

        let first = smart_with(variables.clone());
        let second = smart_with(variables.clone());

        assert_eq!(
            first.format("{global.theVariable}", &no_args()).unwrap(),
            "val-from-global-source"
        );
        assert_eq!(
            second.format("{global.theVariable}", &no_args()).unwrap(),
            "val-from-global-source"
        );
    }

    /// What .NET reaches through the static `Instance`: a change made after
    /// registration is visible to the registered source.
    #[test]
    fn a_clone_shares_the_storage() {
        let variables = GlobalVariablesSource::new();
        let smart = smart_with(variables.clone());

        variables.add("g", group([("v", Value::from("global-v"))]));
        assert_eq!(smart.format("{g.v}", &no_args()).unwrap(), "global-v");

        variables.add("g", group([("v", Value::from("changed"))]));
        assert_eq!(smart.format("{g.v}", &no_args()).unwrap(), "changed");

        variables.remove("g");
        assert!(smart.format("{g.v}", &no_args()).is_err());
    }

    /// .NET's `Reset_Should_Create_A_New_Instance`: after the reset the
    /// variables are gone, while a handle onto the old storage — .NET's
    /// captured `Instance` reference — still has them.
    #[test]
    fn clearing_empties_every_handle_onto_the_same_storage() {
        let variables = GlobalVariablesSource::new();
        let same_storage = variables.clone();
        let fresh = GlobalVariablesSource::new();

        variables.add("g", group([("v", Value::from("v"))]));
        assert_eq!(same_storage.len(), 1);
        assert!(fresh.is_empty());

        variables.clear();
        assert!(variables.is_empty());
        assert!(same_storage.is_empty());
    }

    #[test]
    fn the_shared_source_answers_the_dictionary_questions() {
        let variables =
            GlobalVariablesSource::from_iter([("g", group([("v", Value::from(1i64))]))]);

        assert_eq!(variables.len(), 1);
        assert!(variables.contains("g"));
        assert!(!variables.contains("nope"));
        assert_eq!(variables.get("g"), Some(Value::Map(group([("v", 1i64)]))));
        assert_eq!(variables.get("nope"), None);
        assert_eq!(variables.snapshot().len(), 1);

        let replaced = variables.add("g", group([("v", Value::from(2i64))]));
        assert_eq!(replaced, Some(Value::Map(group([("v", 1i64)]))));
        assert_eq!(
            variables.remove("g"),
            Some(Value::Map(group([("v", 2i64)])))
        );
        assert_eq!(variables.remove("g"), None);
    }

    /// The two sources answer selectors the same way — .NET's
    /// `GlobalVariablesSource` *is* a `PersistentVariablesSource`, over a
    /// different store.
    #[test]
    fn both_sources_resolve_alike() {
        let owned = smart_with(source());
        let shared = smart_with(GlobalVariablesSource::from(source()));

        for template in [
            "{global.group.groupString}",
            "{global.topInteger}",
            "{global.topString}",
        ] {
            assert_eq!(
                owned.format(template, &no_args()).unwrap(),
                shared.format(template, &no_args()).unwrap(),
                "{template}"
            );
        }
        assert!(shared.format("{global.missing}", &no_args()).is_err());
    }

    /// A registered source is shared across threads like any other, which is
    /// what .NET's `Parallel_Load_By_Adding_Variables_To_Instance` is after —
    /// except that here the lock is not a setting.
    #[test]
    fn the_shared_source_takes_writes_from_several_threads() {
        let variables = GlobalVariablesSource::new();
        std::thread::scope(|scope| {
            for index in 0..8i64 {
                let variables = variables.clone();
                scope.spawn(move || {
                    variables.add(format!("g{index:04}"), group([("v", Value::from(index))]));
                });
            }
        });

        assert_eq!(variables.len(), 8);
        let smart = smart_with(variables);
        assert_eq!(smart.format("{g0007.v}", &no_args()).unwrap(), "7");
    }
}
