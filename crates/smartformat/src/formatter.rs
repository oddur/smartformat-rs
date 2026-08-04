//! The formatting engine.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/SmartFormatter.cs`,
//! `Evaluator.cs`, `Core/Formatting/FormattingInfo.cs` and
//! `Extensions/DefaultFormatter.cs`.
//!
//! [`SmartFormatter`] walks a parsed [`Format`], resolves each placeholder's
//! selector chain through the [`SourceRegistry`], and hands the resolved value
//! to a [`Formatter`] extension, which writes the text.

use std::borrow::Cow;
use std::cell::Cell;

use crate::error::Error;
#[cfg(feature = "regex-formatters")]
use crate::extensions::ismatch::IsMatchFormatter;
#[cfg(feature = "plural")]
use crate::extensions::PluralLocalizationFormatter;
use crate::extensions::{
    ChooseFormatter, ConditionalFormatter, ListFormatter, LocalizationFormatter,
    LocalizationProvider, NullFormatter, RegisterError, SubStringFormatter, TemplateFormatter,
};
use crate::fmt::culture::{self, CultureData};
#[cfg(feature = "time")]
use crate::fmt::date;
use crate::fmt::number::{self, Number};
use crate::fmt::{utf16_len, FormatSpecError};
use crate::parsing::chars::ALIGNMENT_OPERATOR;
use crate::parsing::{Format, FormatItem, Parser, ParserSettings, Placeholder, Selector};
use crate::registry;
use crate::settings::{CaseSensitivity, ErrorAction, SmartSettings};
use crate::sources::variables::{GlobalVariablesSource, PersistentVariablesSource};
use crate::sources::{SelectorInfo, SourceRegistry};
use crate::value::Value;

/// The value a nameless placeholder resolves to when there is nothing in scope.
static NULL: Value = Value::Null;

/// How deep a scope chain [`FormattingInfo::write_child`] builds on the stack
/// before it falls back to the heap. One scope per enclosing placeholder plus
/// the two a list item pushes; templates nest a handful deep at most, and the
/// array is eight pointers.
const INLINE_SCOPES: usize = 8;

// ---------------------------------------------------------------------------
// Formatter extensions
// ---------------------------------------------------------------------------

/// Formats a value into text, mirroring .NET `IFormatter`.
///
/// A formatter is selected either by its [`name`](Formatter::name), when the
/// placeholder names one (`{0:plural:...}`), or by auto-detection, when it does
/// not.
pub trait Formatter: Send + Sync {
    /// The name that selects this formatter in a placeholder.
    fn name(&self) -> &str;

    /// Whether this formatter may be chosen for placeholders that name no
    /// formatter (.NET `IFormatter.CanAutoDetect`).
    fn can_auto_detect(&self) -> bool {
        true
    }

    /// Writes the formatted value, or returns `Ok(false)` if this formatter
    /// cannot handle the value, which lets the next one try
    /// (.NET `IFormatter.TryEvaluateFormat`).
    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error>;

    /// Whether this is the formatter of last resort. In `string.Format`
    /// compatibility mode it is the only one that runs, which is .NET's
    /// `_formatterExtensions.First(fe => fe is DefaultFormatter)`.
    fn is_default_formatter(&self) -> bool {
        false
    }

    /// This formatter as an [`Any`], so that a caller who registered it can
    /// find it again and set its knobs — the port's stand-in for .NET's
    /// `GetFormatterExtension<T>()`, which
    /// [`FormatterRegistry::get_mut`] is built on.
    ///
    /// A `dyn Formatter` cannot be downcast the ordinary way: `Any` would have
    /// to be a supertrait, and coercing `&mut dyn Formatter` to `&mut dyn Any`
    /// needs trait upcasting, stable well past this crate's MSRV. Handing the
    /// coercion to the implementer costs one line and asks nothing of the
    /// compiler.
    ///
    /// Every formatter this crate ships answers `Some(self)`, and a formatter
    /// of your own wants to as well; the default is `None` only so that adding
    /// the method breaks nobody. A formatter that declines is invisible to
    /// [`get_mut`](FormatterRegistry::get_mut) — [`register_localization`] and
    /// [`register_template`] would then register a second extension that name
    /// lookup could never reach.
    ///
    /// [`Any`]: std::any::Any
    /// [`register_localization`]: SmartFormatter::register_localization
    /// [`register_template`]: SmartFormatter::register_template
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }
}

/// The rank .NET's `WellKnownExtensionTypes.Formatters` gives each extension,
/// keyed by that extension's default name. `XElementFormatter` (5000) is the
/// only rank left out, because this port has no counterpart for it; nothing is
/// inserted between the others.
///
/// `time` is listed whether or not the feature that builds a `TimeFormatter` is
/// on: the table describes .NET's order, not this build's registry, and a
/// caller who named a formatter of their own `time` should get .NET's slot for
/// it either way.
const WELL_KNOWN_RANKS: [(&str, u32); 11] = [
    ("list", 1000),
    ("plural", 2000),
    ("cond", 3000),
    ("time", 4000),
    ("ismatch", 6000),
    ("isnull", 7000),
    ("L", 8000),
    ("t", 9000),
    ("choose", 10000),
    ("substr", 11000),
    ("d", 12000),
];

/// The rank of the extension of that name, or `None` for a name .NET's table
/// does not hold. Ordinal, as .NET's `Dictionary<string, int>` comparer is.
fn well_known_rank(name: &str) -> Option<u32> {
    WELL_KNOWN_RANKS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, rank)| *rank)
}

/// The ordered list of [`Formatter`] extensions a [`SmartFormatter`] consults.
pub struct FormatterRegistry {
    formatters: Vec<Box<dyn Formatter>>,
}

impl FormatterRegistry {
    /// An empty registry. Every placeholder fails until a formatter is added.
    pub fn empty() -> Self {
        Self {
            formatters: Vec::new(),
        }
    }

    /// The default formatters, in the order .NET's `CreateDefaultSmartFormat`
    /// ends up with.
    ///
    /// .NET adds its extensions in one call and lets `WellKnownExtensionTypes`
    /// sort them by a fixed rank: [`ListFormatter`] (1000),
    /// `PluralLocalizationFormatter` (2000), [`ConditionalFormatter`] (3000),
    /// `IsMatchFormatter` (6000), [`NullFormatter`] (7000),
    /// [`ChooseFormatter`] (10000), [`SubStringFormatter`] (11000),
    /// [`DefaultFormatter`] (12000). The one rank in between that belongs to an
    /// extension this port does not have is `XElementFormatter` (5000).
    ///
    /// Three ported extensions are deliberately absent, because .NET's
    /// `CreateDefaultSmartFormat` leaves them out too: `TimeFormatter` (4000),
    /// `LocalizationFormatter` (8000) and `TemplateFormatter` (9000) are all
    /// useless until they are given a language, a provider or a template. See
    /// [`SmartFormatter::register_template`],
    /// [`SmartFormatter::register_localization`] and [`add`](Self::add), which
    /// slots any of them where this list would have held it.
    ///
    /// The order is observable: [`ListFormatter`],
    /// `PluralLocalizationFormatter` and [`ConditionalFormatter`] all
    /// auto-detect a `|`-separated format, so the first of the three decides
    /// what `{0:a|b}` means. `ListFormatter` ranking first is what makes
    /// `{0:one|many}` on a list a list and not a plural.
    ///
    /// [`ListFormatter`]: crate::extensions::ListFormatter
    /// [`NullFormatter`]: crate::extensions::NullFormatter
    /// [`SubStringFormatter`]: crate::extensions::SubStringFormatter
    pub fn new() -> Self {
        let formatters: Vec<Box<dyn Formatter>> = vec![
            Box::new(ListFormatter::new()),
            #[cfg(feature = "plural")]
            Box::new(PluralLocalizationFormatter::new()),
            Box::new(ConditionalFormatter::new()),
            #[cfg(feature = "regex-formatters")]
            Box::new(IsMatchFormatter::new()),
            Box::new(NullFormatter::new()),
            Box::new(ChooseFormatter::new()),
            Box::new(SubStringFormatter::new()),
            Box::new(DefaultFormatter),
        ];
        Self { formatters }
    }

    /// Adds a formatter, which is consulted before the always-last
    /// [`DefaultFormatter`] if one is registered.
    pub fn insert(&mut self, index: usize, formatter: Box<dyn Formatter>) {
        self.formatters.insert(index, formatter);
    }

    /// Adds a formatter at the position .NET's `WellKnownExtensionTypes` gives
    /// it (`Registry.AddExtensions`), so that a formatter added to the default
    /// registry ends up where `CreateDefaultSmartFormat` would have put it: a
    /// `TemplateFormatter` after [`NullFormatter`] and before
    /// [`ChooseFormatter`], whatever else is registered.
    ///
    /// A formatter .NET does not know is appended, exactly as there — which
    /// puts it *after* [`DefaultFormatter`], where it never runs, so a custom
    /// formatter wants [`insert`](Self::insert) instead. .NET has the same
    /// trap.
    ///
    /// [`NullFormatter`]: crate::extensions::NullFormatter
    pub fn add(&mut self, formatter: Box<dyn Formatter>) {
        let index = self.index_to_insert(formatter.name());
        self.formatters.insert(index, formatter);
    }

    /// Where a formatter of that name belongs, a port of
    /// `WellKnownExtensionTypes.GetIndexToInsert`: after the last formatter
    /// ranked at or before it, or at the end when neither is well known.
    ///
    /// .NET keys the rank table on the CLR type; a port has no type name to
    /// key on and uses the formatter's name instead, which is the same thing
    /// for a formatter that was not renamed.
    fn index_to_insert(&self, name: &str) -> usize {
        registry::index_to_insert(
            self.formatters
                .iter()
                .map(|formatter| well_known_rank(formatter.name())),
            well_known_rank(name),
        )
    }

    /// The registered formatter of type `T`, if there is one — .NET's
    /// `SmartFormatter.GetFormatterExtension<T>()`.
    ///
    /// This is how a knob that is not a setting gets turned: .NET's
    /// `IFormatter` implementations carry properties of their own
    /// (`ListFormatter.SplitChar`, `SubStringFormatter.NullDisplayString`,
    /// `IsMatchFormatter.RegexOptions`), and the way to reach them is to find
    /// the instance the registry already owns rather than to build a second
    /// one. [`SmartFormatter::register_template`] and
    /// [`SmartFormatter::register_localization`] are both this call plus a
    /// registration when it misses.
    ///
    /// The first formatter of that type wins, as name lookup does. A formatter
    /// that does not implement [`Formatter::as_any_mut`] is not found.
    ///
    /// ```
    /// use smartformat::extensions::ListFormatter;
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let mut smart = SmartFormatter::default();
    /// smart
    ///     .formatters_mut()
    ///     .get_mut::<ListFormatter>()
    ///     .expect("the default registry holds one")
    ///     .set_split_char(',')
    ///     .unwrap();
    ///
    /// let args = Value::List(vec![Value::List(vec![Value::Int(1), Value::Int(2)])]);
    /// assert_eq!(smart.format("{0:list:{}, and }", &args).unwrap(), "1 and 2");
    /// ```
    pub fn get_mut<T: Formatter + 'static>(&mut self) -> Option<&mut T> {
        self.formatters
            .iter_mut()
            .find_map(|formatter| formatter.as_any_mut()?.downcast_mut::<T>())
    }

    pub fn push(&mut self, formatter: Box<dyn Formatter>) {
        self.formatters.push(formatter);
    }

    pub fn len(&self) -> usize {
        self.formatters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.formatters.is_empty()
    }

    /// The registered formatters, in the order they are consulted.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &dyn Formatter> {
        self.formatters.iter().map(AsRef::as_ref)
    }

    fn find(&self, name: &str, case_sensitivity: CaseSensitivity) -> Option<&dyn Formatter> {
        self.formatters
            .iter()
            .map(AsRef::as_ref)
            .find(|formatter| case_sensitivity.eq(formatter.name(), name))
    }

    /// The .NET compatibility-mode formatter: the first
    /// [`DefaultFormatter`] in the registry.
    fn default_formatter(&self) -> Option<&dyn Formatter> {
        self.formatters
            .iter()
            .map(AsRef::as_ref)
            .find(|formatter| formatter.is_default_formatter())
    }
}

impl Default for FormatterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FormatterRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormatterRegistry")
            .field("formatters", &self.formatters.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SmartFormatter
// ---------------------------------------------------------------------------

/// Renders SmartFormat templates.
///
/// ```
/// use smartformat::{SmartFormatter, Value};
///
/// let smart = SmartFormatter::default();
/// let args = Value::List(vec![Value::from("Joe"), Value::from(42i64)]);
/// assert_eq!(smart.format("{0} is {1}", &args).unwrap(), "Joe is 42");
/// ```
#[derive(Debug)]
pub struct SmartFormatter {
    settings: SmartSettings,
    parser: Parser,
    sources: SourceRegistry,
    formatters: FormatterRegistry,
}

impl SmartFormatter {
    /// A formatter with the M1 source and formatter extensions registered.
    pub fn new(settings: SmartSettings) -> Self {
        let parser_settings = ParserSettings {
            error_action: settings.parse_error_action,
            string_format_compatibility: settings.string_format_compatibility,
            ..ParserSettings::default()
        };
        Self::with_parser_settings(settings, parser_settings)
    }

    /// Like [`new`](Self::new), but with parser settings that are not derived
    /// from [`SmartSettings`], such as a custom selector character set.
    ///
    /// The two settings a parser and a formatter share —
    /// [`ParserSettings::error_action`] and
    /// [`ParserSettings::string_format_compatibility`] — are taken from the
    /// passed parser settings and copied back over the corresponding
    /// [`SmartSettings`] fields, so [`settings()`](Self::settings) and
    /// [`parser()`](Self::parser)`.settings()` can never disagree.
    pub fn with_parser_settings(
        mut settings: SmartSettings,
        parser_settings: ParserSettings,
    ) -> Self {
        // .NET keeps one `SmartSettings` that owns the `ParserSettings`, so
        // the two views are the same object; here the parser settings win and
        // are mirrored into the formatter's copy.
        settings.parse_error_action = parser_settings.error_action;
        settings.string_format_compatibility = parser_settings.string_format_compatibility;

        Self {
            settings,
            parser: Parser::new(parser_settings),
            sources: SourceRegistry::new(),
            formatters: FormatterRegistry::new(),
        }
    }

    pub fn settings(&self) -> &SmartSettings {
        &self.settings
    }

    pub fn parser(&self) -> &Parser {
        &self.parser
    }

    pub fn sources(&self) -> &SourceRegistry {
        &self.sources
    }

    pub fn sources_mut(&mut self) -> &mut SourceRegistry {
        &mut self.sources
    }

    pub fn formatters(&self) -> &FormatterRegistry {
        &self.formatters
    }

    pub fn formatters_mut(&mut self) -> &mut FormatterRegistry {
        &mut self.formatters
    }

    /// Parses `template` and registers it under `name`, so that
    /// `{:t:<name>}` renders it (.NET `TemplateFormatter.Register`).
    ///
    /// The first call also adds the [`TemplateFormatter`] itself, at .NET's
    /// rank for it — after `isnull`, before `choose`. .NET's
    /// `CreateDefaultSmartFormat` leaves the extension out, so a formatter
    /// that is never handed a template never carries one either.
    ///
    /// The template is parsed with *this* formatter's [`parser`](Self::parser),
    /// and the registry is matched with this formatter's
    /// [`case_sensitive`](crate::SmartSettings::case_sensitive) setting as it
    /// stands at the first call — .NET fixes the comparer in `Initialize` in
    /// the same way. Going through
    /// [`TemplateFormatter::register`](crate::extensions::TemplateFormatter::register)
    /// by hand is what a caller with a second parser would have to do, and is
    /// the only way to get a template parsed with settings the renderer does
    /// not share.
    ///
    /// Fails if `template` does not parse, or if `name` is already registered
    /// — .NET's `Dictionary.Add` throws rather than overwriting.
    ///
    /// ```
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let mut smart = SmartFormatter::default();
    /// smart.register_template("firstLast", "{First} {Last}").unwrap();
    ///
    /// let person = Value::Map(
    ///     [
    ///         ("First".to_owned(), Value::from("Scott")),
    ///         ("Last".to_owned(), Value::from("Rippey")),
    ///     ]
    ///     .into_iter()
    ///     .collect(),
    /// );
    /// assert_eq!(smart.format("{:t:firstLast}", &person).unwrap(), "Scott Rippey");
    /// ```
    pub fn register_template(
        &mut self,
        name: impl Into<String>,
        template: &str,
    ) -> Result<(), RegisterError> {
        if self.formatters.get_mut::<TemplateFormatter>().is_none() {
            let formatter = TemplateFormatter::new(self.settings.case_sensitive);
            self.formatters.add(Box::new(formatter));
        }
        // Disjoint fields: the parser is read while the registry is written.
        let parser = &self.parser;
        self.formatters
            .get_mut::<TemplateFormatter>()
            .expect("a template formatter was just added")
            .register(parser, name, template)
    }

    /// Registers `provider`, so that `{:L:<key>}` renders the localized string
    /// for `<key>` (.NET `AddExtensions(new LocalizationFormatter())` plus
    /// `SmartSettings.Localization.LocalizationProvider`).
    ///
    /// The first call adds the [`LocalizationFormatter`] itself, at .NET's rank
    /// for it — after `isnull`, before `t`. .NET's `CreateDefaultSmartFormat`
    /// leaves the extension out, because a formatter with no provider has
    /// nothing to translate with.
    ///
    /// A second call replaces the provider of the formatter already registered
    /// rather than adding another one, which is what .NET's single
    /// `LocalizationProvider` setting amounts to — and the only useful reading,
    /// since name lookup would never reach a second formatter called `L`.
    /// Replacing empties the formatter's parse cache.
    ///
    /// Localized strings are parsed with *this* formatter's
    /// [`parser`](Self::parser), lazily, the first time each one is rendered,
    /// and the cache is keyed with this formatter's
    /// [`case_sensitive`](crate::SmartSettings::case_sensitive) setting as it
    /// stands at the first call.
    ///
    /// ```
    /// use smartformat::extensions::localization::HashMapLocalizationProvider;
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let provider: HashMapLocalizationProvider = [
    ///     ("", "Hello", "Hello"),
    ///     ("de", "Hello", "Hallo"),
    /// ]
    /// .into_iter()
    /// .collect();
    ///
    /// let mut smart = SmartFormatter::default();
    /// smart.register_localization(Box::new(provider));
    ///
    /// let none = Value::Null;
    /// assert_eq!(smart.format("{:L:Hello}", &none).unwrap(), "Hello");
    /// assert_eq!(smart.format("{:L(de):Hello}", &none).unwrap(), "Hallo");
    /// ```
    ///
    /// [`LocalizationFormatter`]: crate::extensions::LocalizationFormatter
    pub fn register_localization(&mut self, provider: Box<dyn LocalizationProvider>) {
        if let Some(formatter) = self.formatters.get_mut::<LocalizationFormatter>() {
            formatter.set_provider(provider);
            return;
        }
        let formatter = LocalizationFormatter::new(&self.parser, provider)
            .with_case_sensitivity(self.settings.case_sensitive);
        self.formatters.add(Box::new(formatter));
    }

    /// Registers `source`, so that `{groupName.variableName}` resolves against
    /// its groups without them being passed as arguments (.NET
    /// `AddExtensions(new PersistentVariablesSource())`).
    ///
    /// The source is slotted where .NET's `WellKnownExtensionTypes.Sources`
    /// ranks it — ahead of every other source, so a *group name* wins over a
    /// selector any of them would have answered. Nothing else changes: a
    /// selector this source declines is answered exactly as it is without it,
    /// map arguments included. `CreateDefaultSmartFormat` does not register it,
    /// here or in .NET.
    ///
    /// The source owns its groups and the registry hands out no way back to it,
    /// so fill it before registering. [`register_global_variables`] takes a
    /// handle to shared storage that stays writable afterwards.
    ///
    /// ```
    /// use smartformat::sources::variables::{self, PersistentVariablesSource};
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let mut variables = PersistentVariablesSource::new();
    /// variables.add("app", variables::group([("name", Value::from("Acme"))]));
    ///
    /// let mut smart = SmartFormatter::default();
    /// smart.register_variables(variables);
    /// assert_eq!(smart.format("{app.name}", &Value::Null).unwrap(), "Acme");
    /// ```
    ///
    /// [`register_global_variables`]: Self::register_global_variables
    pub fn register_variables(&mut self, source: PersistentVariablesSource) {
        self.sources.add(Box::new(source));
    }

    /// Registers a handle to shared variable storage, ahead of every other
    /// source and of a [`register_variables`](Self::register_variables) source
    /// (.NET `AddExtensions(GlobalVariablesSource.Instance)`).
    ///
    /// Pass a clone and keep one: groups added through any handle are visible
    /// to every formatter registered with another, which is what .NET's
    /// `static` store gives its singleton.
    ///
    /// ```
    /// use smartformat::sources::variables::{self, GlobalVariablesSource};
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let variables = GlobalVariablesSource::new();
    /// let mut smart = SmartFormatter::default();
    /// smart.register_global_variables(variables.clone());
    ///
    /// // Filled after registration, which the persistent source cannot do.
    /// variables.add("app", variables::group([("name", Value::from("Acme"))]));
    /// assert_eq!(smart.format("{app.name}", &Value::Null).unwrap(), "Acme");
    /// ```
    pub fn register_global_variables(&mut self, source: GlobalVariablesSource) {
        self.sources.add(Box::new(source));
    }

    /// Parses a template once, for repeated formatting.
    ///
    /// Syntax errors are handled per
    /// [`SmartSettings::parse_error_action`]; only [`ErrorAction::Error`]
    /// returns [`Error::Parse`].
    pub fn parse(&self, template: &str) -> Result<Format, Error> {
        self.parser.parse(template)
    }

    /// Renders `template` with the invariant culture.
    pub fn format(&self, template: &str, args: &Value) -> Result<String, Error> {
        self.format_with_culture(template, args, culture::invariant())
    }

    /// Renders `template` with the given culture data.
    pub fn format_with_culture(
        &self,
        template: &str,
        args: &Value,
        culture: &CultureData,
    ) -> Result<String, Error> {
        let format = self.parse(template)?;
        self.format_parsed_with_culture(&format, args, culture)
    }

    /// Renders `template` with the culture of that name — `""` for the
    /// invariant culture, `"de-DE"`, `"is"`, … — matched case-insensitively as
    /// .NET's `GetCultureInfo` does. A name may carry an alternate sort order
    /// after an `_`, which .NET's data resolution ignores and so do we:
    /// `"en_US"` is the language `en` sorted the American way, not `en-US`.
    ///
    /// Fails with [`Error::UnknownCulture`] when the name is one .NET rejects
    /// or when no data is shipped for it;
    /// [`fmt::culture::get`](crate::fmt::culture::get) is the same lookup
    /// without the error, for a caller that would rather fall back, and
    /// documents both rules.
    ///
    /// ```
    /// use smartformat::{SmartFormatter, Value};
    ///
    /// let smart = SmartFormatter::default();
    /// let args = Value::List(vec![Value::Float(1234.5)]);
    /// assert_eq!(
    ///     smart.format_with_culture_name("{0:N2}", &args, "de-DE").unwrap(),
    ///     "1.234,50"
    /// );
    /// // `en_US` is `en`, whose currency symbol is the placeholder `¤`;
    /// // `en-US`, the culture, spends dollars.
    /// assert_eq!(
    ///     smart.format_with_culture_name("{0:C2}", &args, "en_US").unwrap(),
    ///     "\u{a4}1,234.50"
    /// );
    /// assert_eq!(
    ///     smart.format_with_culture_name("{0:C2}", &args, "en-US").unwrap(),
    ///     "$1,234.50"
    /// );
    /// assert!(smart.format_with_culture_name("{0}", &args, "xx-XX").is_err());
    /// ```
    pub fn format_with_culture_name(
        &self,
        template: &str,
        args: &Value,
        culture: &str,
    ) -> Result<String, Error> {
        self.format_with_culture(template, args, culture_by_name(culture)?)
    }

    /// Renders a template parsed by [`parse`](Self::parse), with the culture of
    /// that name. See [`format_with_culture_name`](Self::format_with_culture_name).
    pub fn format_parsed_with_culture_name(
        &self,
        format: &Format,
        args: &Value,
        culture: &str,
    ) -> Result<String, Error> {
        self.format_parsed_with_culture(format, args, culture_by_name(culture)?)
    }

    /// Renders a template parsed by [`parse`](Self::parse), with the invariant
    /// culture.
    pub fn format_parsed(&self, format: &Format, args: &Value) -> Result<String, Error> {
        self.format_parsed_with_culture(format, args, culture::invariant())
    }

    /// Renders a template parsed by [`parse`](Self::parse), with the given
    /// culture data.
    pub fn format_parsed_with_culture(
        &self,
        format: &Format,
        args: &Value,
        culture: &CultureData,
    ) -> Result<String, Error> {
        // .NET passes the arguments as a list and formats against its first
        // element, so a list argument is the positional argument set and any
        // other value is a single argument.
        let arg_list: &[Value] = match args {
            Value::List(items) => items,
            single => std::slice::from_ref(single),
        };
        // .NET `SmartFormatter.ExecuteFormattingAction`:
        // `var current = args.Count > 0 ? args[0] : args;` — an empty argument
        // list is its own current value, not null.
        let current = arg_list.first().unwrap_or(args);

        let engine = Engine {
            smart: self,
            args: arg_list,
            culture,
            culture_override: Cell::new(None),
            base: &format.raw,
            collection_index: Cell::new(NO_COLLECTION_INDEX),
        };
        let mut output = String::new();
        engine.write_format(format, &[current], 0, &mut output)?;
        Ok(output)
    }
}

impl Default for SmartFormatter {
    fn default() -> Self {
        Self::new(SmartSettings::default())
    }
}

/// The culture of that name, or [`Error::UnknownCulture`].
fn culture_by_name(name: &str) -> Result<&'static CultureData, Error> {
    culture::get(name).ok_or_else(|| Error::UnknownCulture {
        name: name.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// The evaluator
// ---------------------------------------------------------------------------

/// The [`collection_index`](FormattingInfo::collection_index) outside any list
/// iteration, .NET `ListFormatter.CollectionIndex`'s own sentinel: `{Index}` on
/// an enumerable renders `-1` there.
pub const NO_COLLECTION_INDEX: i32 = -1;

/// One format call, ported from .NET `Evaluator` plus `FormatDetails`.
struct Engine<'a> {
    smart: &'a SmartFormatter,
    args: &'a [Value],
    /// The culture the call was made with (.NET `FormatDetails.Provider`),
    /// until [`culture_override`](Self::culture_override) replaces it.
    culture: &'a CultureData,
    /// The culture a formatter switched to with
    /// [`FormattingInfo::set_culture`], which belongs to the whole call because
    /// .NET's `FormatDetails` does.
    ///
    /// A `Cell<&'a CultureData>` would say the same thing in one field, but it
    /// would make `Engine<'a>` *invariant* in `'a`, and the engine leans on its
    /// covariance: [`FormattingInfo::format_as_child`] renders a `Format` that
    /// outlives nothing but the call — a translation out of a parse cache —
    /// through a `FormattingInfo` shorter-lived than the engine it borrows.
    /// `&'static` is what [`fmt::culture`](crate::fmt::culture) hands out, so
    /// the split costs a caller nothing.
    culture_override: Cell<Option<&'static CultureData>>,
    /// The whole template, quoted by error messages (.NET
    /// `FormatItem.BaseString`).
    base: &'a str,
    /// The index of the list item being formatted, which the `list` formatter
    /// keeps up to date and the `{Index}` selector reads
    /// (.NET `ListFormatter.CollectionIndex`).
    ///
    /// .NET holds this in a `static` — an `AsyncLocal` in thread-safe mode —
    /// because its formatter extensions are stateless singletons shared by
    /// every call. Here it belongs to the call, which is the same thing seen
    /// from a template, minus .NET's two hazards: two threads formatting at
    /// once cannot mix their indexes up, and an index a failed call left behind
    /// cannot leak into the next one (see
    /// [`ListFormatter`](crate::extensions::list::ListFormatter)).
    collection_index: Cell<i32>,
}

impl<'e> Engine<'e> {
    /// The culture in force right now: the one the call was made with, or the
    /// one a formatter switched to.
    fn culture(&self) -> &'e CultureData {
        self.culture_override.get().unwrap_or(self.culture)
    }

    /// .NET `Evaluator.WriteFormat`. `scopes` holds the current value of every
    /// enclosing format, innermost last.
    fn write_format<'v>(
        &self,
        format: &'v Format,
        scopes: &[&'v Value],
        alignment: i32,
        output: &mut String,
    ) -> Result<(), Error>
    where
        'e: 'v,
    {
        for item in &format.items {
            match item {
                // Literals respect the alignment of the format they are in,
                // as in .NET.
                FormatItem::Literal(literal) => {
                    // .NET resolves escape sequences here, in
                    // `LiteralText.AsSpan()`, and throws if one resolves to
                    // nothing. A literal that is never written — the format of
                    // `{0:0.00}` reaches the value as a specifier instead —
                    // never rejects its escape sequences.
                    if let Some(message) = &literal.escape_error {
                        return Err(Error::Escape {
                            message: message.clone(),
                            position: self.utf16_position(literal.start),
                        });
                    }
                    write_aligned(
                        output,
                        &literal.text,
                        alignment,
                        self.smart.settings.alignment_fill_character,
                    )
                }
                FormatItem::Placeholder(placeholder) => {
                    self.write_placeholder(placeholder, scopes, output)?
                }
            }
        }
        Ok(())
    }

    /// The position of a byte offset into the template in UTF-16 code units,
    /// which is the unit .NET reports positions in.
    fn utf16_position(&self, offset: usize) -> usize {
        // The offset comes from the tree and always sits on a character
        // boundary of the template it was parsed from. A `Format` built by
        // hand can only make the position wrong, never panic.
        match self.base.get(..offset) {
            Some(prefix) => utf16_len(prefix),
            None => offset,
        }
    }

    /// .NET `Evaluator.EvaluatePlaceholder` plus `InvokeFormatters`.
    fn write_placeholder<'v>(
        &self,
        placeholder: &'v Placeholder,
        scopes: &[&'v Value],
        output: &mut String,
    ) -> Result<(), Error>
    where
        'e: 'v,
    {
        let value = match self.evaluate_selectors(placeholder, scopes) {
            Ok(value) => value,
            Err(error) => return self.handle_format_error(placeholder, error, output),
        };

        match self.invoke_formatters(placeholder, value.as_ref(), scopes, output) {
            Ok(()) => Ok(()),
            Err(error) => self.handle_format_error(placeholder, error, output),
        }
    }

    /// .NET `Evaluator.EvaluateSelectors`.
    fn evaluate_selectors<'v>(
        &self,
        placeholder: &'v Placeholder,
        scopes: &[&'v Value],
    ) -> Result<Cow<'v, Value>, Error>
    where
        'e: 'v,
    {
        let mut current: Cow<'v, Value> = Cow::Borrowed(scopes.last().copied().unwrap_or(&NULL));
        // .NET only falls back to the enclosing scopes for the first selector
        // that fails; the flag is not reset by a successful selector.
        let mut first_selector = true;

        for selector in &placeholder.selectors {
            if skip_selector(selector) {
                continue;
            }

            let resolved = match current {
                Cow::Borrowed(value) => self.resolve(value, selector, placeholder),
                Cow::Owned(ref value) => self
                    .resolve(value, selector, placeholder)
                    .map(|next| Cow::Owned(next.into_owned())),
            };

            let resolved = match resolved {
                Some(value) => value,
                None if first_selector => {
                    first_selector = false;
                    match self.resolve_in_scopes(selector, placeholder, scopes) {
                        Some(value) => value,
                        None => return Err(self.selector_error(selector)),
                    }
                }
                None => return Err(self.selector_error(selector)),
            };

            current = resolved;
        }

        Ok(current)
    }

    fn resolve<'v>(
        &self,
        current: &'v Value,
        selector: &'v Selector,
        placeholder: &'v Placeholder,
    ) -> Option<Cow<'v, Value>>
    where
        'e: 'v,
    {
        self.smart.sources.evaluate(SelectorInfo {
            current,
            selector,
            placeholder,
            args: self.args,
            settings: &self.smart.settings,
            collection_index: self.collection_index.get(),
        })
    }

    /// .NET `Evaluator.HandleNestedScope`: a selector that no source can handle
    /// in the current scope is retried against the enclosing scopes,
    /// innermost first.
    fn resolve_in_scopes<'v>(
        &self,
        selector: &'v Selector,
        placeholder: &'v Placeholder,
        scopes: &[&'v Value],
    ) -> Option<Cow<'v, Value>>
    where
        'e: 'v,
    {
        scopes
            .iter()
            .rev()
            .find_map(|scope| self.resolve(scope, selector, placeholder))
    }

    /// .NET `Evaluator.InvokeFormatters` plus `Registry.InvokeFormatterExtensions`.
    fn invoke_formatters<'v>(
        &self,
        placeholder: &'v Placeholder,
        current: &'v Value,
        scopes: &'v [&'v Value],
        output: &'v mut String,
    ) -> Result<(), Error>
    where
        'e: 'v,
    {
        let mut info = FormattingInfo {
            engine: self,
            scopes,
            placeholder,
            format: placeholder.format.as_ref(),
            current,
            alignment: placeholder.alignment,
            output,
        };

        // Auto-detection is the only path that consults more than one
        // formatter: compatibility mode bypasses every extension but
        // DefaultFormatter, the auto-detecting ones included, and a
        // placeholder that names a formatter reaches that one alone.
        let picked = if self.smart.settings.string_format_compatibility {
            self.smart.formatters.default_formatter()
        } else if placeholder.formatter_name.is_empty() {
            for formatter in &self.smart.formatters.formatters {
                if !formatter.can_auto_detect() {
                    continue;
                }
                if formatter.try_evaluate_format(&mut info)? {
                    return Ok(());
                }
            }
            return Err(self.no_formatter_error(placeholder));
        } else {
            self.smart.formatters.find(
                &placeholder.formatter_name,
                self.smart.settings.case_sensitive,
            )
        };

        // .NET reports a missing formatter, a formatter that declined the
        // value, and no auto-detecting formatter at all with one message.
        let handled = match picked {
            Some(formatter) => formatter.try_evaluate_format(&mut info)?,
            None => false,
        };
        if handled {
            Ok(())
        } else {
            Err(self.no_formatter_error(placeholder))
        }
    }

    /// .NET `Evaluator.FormatError`: the settings decide whether an error
    /// fails the call or is recovered from.
    fn handle_format_error(
        &self,
        placeholder: &Placeholder,
        error: Error,
        output: &mut String,
    ) -> Result<(), Error> {
        let fill = self.smart.settings.alignment_fill_character;

        // Before it looks at the error action, .NET builds its error event
        // from `Placeholder.RawText`, which rebuilds the placeholder and so
        // reads `FormatterOptions`. Options that cannot be resolved throw
        // there, replacing whatever error was being handled — whatever the
        // error action is.
        if let Some(message) = &placeholder.formatter_options_error {
            return Err(Error::Escape {
                message: message.clone(),
                position: error_position(placeholder),
            });
        }

        match self.smart.settings.format_error_action {
            // .NET rethrows what it caught as a `FormattingException` unless it
            // already is one, so an escape sequence that failed inside a
            // placeholder becomes an ordinary formatting error here.
            ErrorAction::Error => Err(match error {
                Error::Escape { message, .. } => Error::Format {
                    message,
                    position: error_position(placeholder),
                },
                error => error,
            }),
            ErrorAction::Ignore => Ok(()),
            ErrorAction::OutputErrorInResult => {
                write_aligned(output, &error_message(&error), placeholder.alignment, fill);
                Ok(())
            }
            ErrorAction::MaintainTokens => {
                // .NET writes `Placeholder.RawText`, which `Placeholder`
                // overrides to rebuild the placeholder from its parsed parts,
                // rather than the text it was parsed from.
                write_aligned(
                    output,
                    &placeholder.to_string(),
                    placeholder.alignment,
                    fill,
                );
                Ok(())
            }
        }
    }

    /// .NET `Evaluator.EvaluateSelectors`, whose `FormattingException` is
    /// positioned at the selector that could not be evaluated.
    fn selector_error(&self, selector: &Selector) -> Error {
        self.formatting_error(
            &format!(
                "No source extension could handle the selector named \"{}\"",
                selector.text
            ),
            self.utf16_position(selector.start),
        )
    }

    /// .NET `Evaluator.InvokeFormatters`, which reports a missing formatter,
    /// a formatter that declined the value, and no auto-detecting formatter
    /// at all with one message — and positions it at the *ordinal* index of
    /// the last evaluated selector, not at an offset into the template.
    ///
    /// A placeholder with no selector to report has none: .NET passes `-1`,
    /// which `FormattingInfo.FormattingException` reads as "use the problem
    /// item", and the problem item here is the placeholder's format. So
    /// `{:nope:x}` is reported at 7 — where `x` starts — and not at 0
    /// (probed against 3.6.1). `{:t:…}`, the way every template is named, is a
    /// placeholder of exactly that shape.
    fn no_formatter_error(&self, placeholder: &Placeholder) -> Error {
        let index = placeholder
            .selectors
            .iter()
            .rfind(|selector| !skip_selector(selector))
            .map_or_else(
                || self.utf16_position(error_position(placeholder)),
                |selector| selector.index,
            );
        self.formatting_error("No suitable Formatter could be found", index)
    }

    /// A .NET `FormattingException`, whose `Message` quotes the template and
    /// points at `index` (`FormattingException.Message`).
    ///
    /// `index` is already in the unit .NET counts in — UTF-16 code units of
    /// the template — because a couple of call sites pass .NET something that
    /// is not an offset at all; see
    /// [`FormattingInfo::formatting_error_at_utf16`].
    fn formatting_error(&self, issue: &str, index: usize) -> Error {
        Error::Format {
            message: formatting_exception_message(self.base, issue, index),
            position: index,
        }
    }
}

/// .NET `FormattingException.Message`: the issue, the index it is reported at,
/// then the template and a caret line under it.
///
/// [`ErrorAction::OutputErrorInResult`] writes this into the result, so it has
/// to match .NET byte for byte — which is why the tests that pin one build it
/// from here rather than re-spelling it.
pub(crate) fn formatting_exception_message(base: &str, issue: &str, index: usize) -> String {
    format!(
        "Error parsing format string: {issue} at {index}\n{base}\n{}^",
        "-".repeat(index)
    )
}

/// .NET skips empty selectors (`{0..Length}`) and alignment-only selectors
/// (`{0,10}`).
fn skip_selector(selector: &Selector) -> bool {
    selector.text.is_empty() || selector.operator.starts_with(ALIGNMENT_OPERATOR)
}

/// The index .NET reports for an error inside a placeholder.
fn error_position(placeholder: &Placeholder) -> usize {
    if let Some(format) = &placeholder.format {
        return format.start;
    }
    placeholder
        .selectors
        .last()
        .map_or(placeholder.start, |selector| selector.end)
}

/// What [`ErrorAction::OutputErrorInResult`] writes into the output: the plain
/// message, as .NET writes the exception's `Message`.
fn error_message(error: &Error) -> String {
    match error {
        Error::Format { message, .. }
        | Error::UnsupportedSpec { message, .. }
        | Error::Escape { message, .. }
        // .NET's `ParsingErrors.Message`, which a formatter extension that
        // parses a string of its own lets through.
        | Error::Parse { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

/// Pads `text` to `alignment` columns with `fill`: a positive alignment
/// right-aligns, a negative one left-aligns, exactly like `string.Format`.
fn write_aligned(output: &mut String, text: &str, alignment: i32, fill: char) {
    if alignment == 0 {
        output.push_str(text);
        return;
    }

    // .NET measures the alignment in UTF-16 code units.
    let width = utf16_len(text);
    let filler = alignment.unsigned_abs() as usize;
    let padding = filler.saturating_sub(width);

    if alignment > 0 {
        for _ in 0..padding {
            output.push(fill);
        }
        output.push_str(text);
    } else {
        output.push_str(text);
        for _ in 0..padding {
            output.push(fill);
        }
    }
}

// ---------------------------------------------------------------------------
// FormattingInfo
// ---------------------------------------------------------------------------

/// What a [`Formatter`] extension gets to work with, mirroring .NET
/// `IFormattingInfo`.
pub struct FormattingInfo<'a> {
    engine: &'a Engine<'a>,
    scopes: &'a [&'a Value],
    placeholder: &'a Placeholder,
    format: Option<&'a Format>,
    current: &'a Value,
    alignment: i32,
    output: &'a mut String,
}

impl<'a> FormattingInfo<'a> {
    /// The value to format.
    pub fn current(&self) -> &'a Value {
        self.current
    }

    /// The format after the formatter name, if the placeholder has one. This
    /// is both the format specifier (`{0:D3}`) and the nested format
    /// (`{0:{Name}}`), as in .NET.
    pub fn format(&self) -> Option<&'a Format> {
        self.format
    }

    pub fn placeholder(&self) -> &'a Placeholder {
        self.placeholder
    }

    /// The formatter options in parens: `"m|f"` in `{0:choose(m|f):...}`.
    ///
    /// Fails when the options hold an escape sequence that resolves to
    /// nothing: .NET resolves them in the `Placeholder.FormatterOptions`
    /// getter, so a formatter that never reads its options — the default one,
    /// for `{0:d(a\qb)}` — never rejects them.
    pub fn formatter_options(&self) -> Result<&'a str, Error> {
        match &self.placeholder.formatter_options_error {
            None => Ok(&self.placeholder.formatter_options),
            Some(message) => Err(Error::Escape {
                message: message.clone(),
                position: error_position(self.placeholder),
            }),
        }
    }

    /// The formatter options as they appear in the template, with no escape
    /// sequence resolved (.NET `Placeholder.FormatterOptionsRaw`).
    pub fn formatter_options_raw(&self) -> &'a str {
        &self.placeholder.formatter_options_raw
    }

    pub fn alignment(&self) -> i32 {
        self.alignment
    }

    /// The culture this placeholder formats with (.NET
    /// `FormatDetails.Provider`, when it is a `CultureInfo`).
    ///
    /// It is the culture the format call was made with until a formatter
    /// changes it with [`set_culture`](Self::set_culture).
    pub fn culture(&self) -> &'a CultureData {
        self.engine.culture()
    }

    /// Sets the [`culture`](Self::culture) for the rest of this format call.
    ///
    /// .NET's `LocalizationFormatter` does exactly this — `formattingInfo
    /// .FormatDetails.Provider = cultureInfo` — and `FormatDetails` belongs to
    /// the whole call rather than to the placeholder, so `{:L(de):…}` switches
    /// the culture for *every later placeholder of the template*, not only for
    /// the placeholders of the localized string. Probed against 3.6.1:
    /// `Format(en-US, "{0:N2}|{1:L(de):WeTranslateText}|{0:N2}", 1234.5, "")`
    /// is `1,234.50|Wir übersetzen Text|1.234,50`.
    ///
    /// Nothing puts the old culture back, as nothing does in .NET; a formatter
    /// that wants the change to be local saves and restores it the way
    /// [`set_collection_index`](Self::set_collection_index) is bracketed. The
    /// change dies with the format call: the next one starts from its own
    /// culture again, and so does an isolated render started by
    /// [`format_to_isolated_string`](Self::format_to_isolated_string).
    ///
    /// The culture is `&'static` because that is what
    /// [`fmt::culture::get`](crate::fmt::culture::get) hands out; a caller who
    /// built a [`CultureData`] of their own leaks it or keeps it in a `static`.
    pub fn set_culture(&mut self, culture: &'static CultureData) {
        self.engine.culture_override.set(Some(culture));
    }

    pub fn settings(&self) -> &'a SmartSettings {
        &self.engine.smart.settings
    }

    /// Writes text to the output, applying the placeholder's alignment.
    pub fn write(&mut self, text: &str) {
        let fill = self.engine.smart.settings.alignment_fill_character;
        write_aligned(self.output, text, self.alignment, fill);
    }

    /// Writes text to the output without applying the placeholder's alignment.
    ///
    /// .NET writes the spacers of a list through a `FormattingInfo` of its own
    /// whose `Alignment` it sets to 0, so `{0,5:list:{}|-}` pads the items and
    /// not the `-` between them (`ListFormatter.FormatItems`).
    pub fn write_unaligned(&mut self, text: &str) {
        self.output.push_str(text);
    }

    /// The index of the list item being formatted, or
    /// [`NO_COLLECTION_INDEX`] outside any list
    /// (.NET `ListFormatter.CollectionIndex`).
    ///
    /// This is the ambient state behind the `{Index}` selector, which
    /// [`ListSource`](crate::sources::ListSource) reads through
    /// [`SelectorInfo::collection_index`](crate::sources::SelectorInfo::collection_index).
    pub fn collection_index(&self) -> i32 {
        self.engine.collection_index.get()
    }

    /// Sets the [`collection_index`](Self::collection_index) for the rest of
    /// this format call, until someone sets it again.
    ///
    /// It is a formatter's job to save the old value and put it back when it is
    /// done, the way .NET's `ListFormatter.TryEvaluateFormat` brackets its
    /// iteration, so that a list nested in a list leaves the enclosing index
    /// alone.
    pub fn set_collection_index(&mut self, index: i32) {
        self.engine.collection_index.set(index);
    }

    /// The whole template being rendered, which every error message quotes
    /// (.NET `FormatItem.BaseString`).
    pub fn base_string(&self) -> &'a str {
        self.engine.base
    }

    /// The byte offset into [`base_string`](Self::base_string) that .NET
    /// reports an error from this placeholder at: the start of its format, or
    /// the end of its last selector when it has none
    /// (.NET `Evaluator.InvokeFormatters`).
    pub fn error_position(&self) -> usize {
        error_position(self.placeholder)
    }

    /// A .NET `FormattingException` raised by this formatter
    /// (`IFormattingInfo.FormattingException`).
    ///
    /// The message is .NET's `FormattingException.Message` verbatim: the issue,
    /// the index it is reported at, then the whole template and a caret line.
    /// [`ErrorAction::OutputErrorInResult`] writes that message into the
    /// result, so it has to match byte for byte.
    ///
    /// `index` is a byte offset into [`base_string`](Self::base_string) —
    /// usually [`error_position`](Self::error_position) — and is converted to
    /// the UTF-16 code-unit index .NET counts in.
    ///
    /// Use this only where .NET throws a `FormattingException` itself. Where it
    /// throws something else — a `FormatException`, an `ArgumentException` —
    /// the envelope is added by the evaluator, and only when the error is
    /// thrown on: `OutputErrorInResult` writes the inner exception's bare
    /// message. Such a formatter returns a plain [`Error::Format`] instead.
    pub fn formatting_error(&self, issue: &str, index: usize) -> Error {
        self.engine
            .formatting_error(issue, self.engine.utf16_position(index))
    }

    /// Like [`formatting_error`](Self::formatting_error), but with the index
    /// taken verbatim instead of converted from a byte offset.
    ///
    /// A few .NET call sites pass a number that is not an offset into the
    /// template at all — `PluralLocalizationFormatter` reports the number of
    /// plural words it was given — and the caret then lands wherever that
    /// number happens to point. Reproducing the message means reproducing the
    /// index.
    pub fn formatting_error_at_utf16(&self, issue: &str, index: usize) -> Error {
        self.engine.formatting_error(issue, index)
    }

    /// The error .NET raises as a plain exception — a `FormatException`, an
    /// `ArgumentException`, an `OverflowException` — rather than as a
    /// `FormattingException`.
    ///
    /// Its message is the bare issue, with none of the
    /// [`formatting_error`](Self::formatting_error) envelope, because .NET only
    /// wraps such an exception while rethrowing it:
    /// [`ErrorAction::OutputErrorInResult`] writes the *inner* exception's
    /// message, which is the bare text. `index` is a byte offset into
    /// [`base_string`](Self::base_string), reported in UTF-16 code units as
    /// .NET counts them.
    pub fn plain_error(&self, issue: &str, index: usize) -> Error {
        Error::Format {
            message: issue.to_owned(),
            position: self.engine.utf16_position(index),
        }
    }

    /// [`formatting_error`](Self::formatting_error) at
    /// [`error_position`](Self::error_position), which is where a formatter
    /// reports an error it has no more precise position for. Nearly every one
    /// of them is of that kind.
    pub fn formatting_error_here(&self, issue: &str) -> Error {
        self.formatting_error(issue, self.error_position())
    }

    /// [`plain_error`](Self::plain_error) at
    /// [`error_position`](Self::error_position), the same convenience as
    /// [`formatting_error_here`](Self::formatting_error_here) for the errors
    /// .NET raises as plain exceptions.
    pub fn plain_error_here(&self, issue: &str) -> Error {
        self.plain_error(issue, self.error_position())
    }

    /// What a formatter answers for a value it cannot handle: `Ok(false)` when
    /// the placeholder named no formatter, so that the next extension gets a
    /// turn, and the error `issue` describes when it named this one.
    ///
    /// Every ported extension ends its "can I handle this?" check this way,
    /// because .NET's do: auto-detection is a `return false`, while a formatter
    /// asked for by name that cannot deliver throws a plain `FormatException`.
    /// The evaluator only wraps that while rethrowing, so the message carries
    /// no [`formatting_error`](Self::formatting_error) envelope —
    /// [`ErrorAction::OutputErrorInResult`] writes the bare text (probed
    /// against 3.6.1).
    ///
    /// `issue` is handed the formatter name the placeholder used, which every
    /// one of those messages quotes.
    pub fn decline_or_error(&self, issue: impl FnOnce(&str) -> String) -> Result<bool, Error> {
        let name = &self.placeholder.formatter_name;
        if name.is_empty() {
            return Ok(false);
        }
        Err(self.plain_error_here(&issue(name)))
    }

    /// Renders `format` with `value` as the current scope, appending to the
    /// same output (.NET `IFormattingInfo.FormatAsChild`).
    pub fn format_as_child(&mut self, format: &Format, value: &Value) -> Result<(), Error> {
        let alignment = self.alignment;
        self.write_child(format, &[value], alignment)
    }

    /// Like [`format_as_child`](Self::format_as_child), but for a value that is
    /// not the one this placeholder resolved to — an item of a list, say — and
    /// with an alignment of its own.
    ///
    /// The placeholder's own value stays in the scope chain underneath `value`,
    /// where .NET's chain of parent `FormattingInfo`s keeps it. That only shows
    /// when the two differ, and then it decides what a selector the item cannot
    /// answer falls back to: in `{0:list:{}={1.Index}|, }` the `Index` of the
    /// *second* list is out of range for its last items, and .NET answers those
    /// from the list being iterated, which sits between the item and the
    /// enclosing scopes.
    ///
    /// `alignment` is what the literals of `format` are written with — the
    /// placeholder's own for a list item, 0 for a spacer, as .NET's
    /// `ListFormatter` sets it. Placeholders inside `format` keep the alignment
    /// the parser gave them either way.
    pub fn format_as_child_of_current(
        &mut self,
        format: &Format,
        value: &Value,
        alignment: i32,
    ) -> Result<(), Error> {
        let current = self.current;
        self.write_child(format, &[current, value], alignment)
    }

    /// Renders `format` against `value` into a string of its own, with none of
    /// this call's context but its registries — .NET's
    /// `FormatDetailsPool.Instance.Get().Initialize(_formatter, format,
    /// InitializationObject.ObjectList, null, zsOutput)`, the brand-new
    /// `FormatDetails` `LocalizationFormatter` builds to turn a format with
    /// nested placeholders into a lookup key.
    ///
    /// What "isolated" means, probed against 3.6.1:
    ///
    /// * the output is fresh, and is returned rather than appended;
    /// * there are **no positional arguments** — `{:L(es):{0}}` with one
    ///   argument fails with `No source extension could handle the selector
    ///   named "0"`;
    /// * the **scope chain is reset** to `value` alone, so `{Inner:L:{Outer}}`
    ///   cannot see an `Outer` that only the root argument has;
    /// * errors are handled by [`SmartSettings::format_error_action`] inside
    ///   the isolated render, so under [`ErrorAction::Ignore`] a failing
    ///   placeholder contributes nothing to the key rather than failing the
    ///   call;
    /// * the base string and every error position still refer to the *original*
    ///   template, which is where `format`'s items were parsed from.
    ///
    /// The alignment is this placeholder's, as .NET's `Initialize` takes it
    /// from `format.ParentPlaceholder`: `{,10:L:pa{B}}` builds its key from a
    /// padded `pa` and a padded `B` (probed).
    ///
    /// One deliberate divergence: .NET passes `null` for the new
    /// `FormatDetails`' provider, which sends every culture-sensitive
    /// placeholder inside the isolated render to the ambient
    /// `CultureInfo.CurrentCulture`. There is no ambient culture here, so the
    /// render uses [`culture`](Self::culture) — the call's, or whatever
    /// [`set_culture`](Self::set_culture) last put there. See DESIGN.md,
    /// "Known divergences".
    pub fn format_to_isolated_string(
        &self,
        format: &Format,
        value: &Value,
    ) -> Result<String, Error> {
        let engine = Engine {
            smart: self.engine.smart,
            // .NET `InitializationObject.ObjectList`, an empty argument list.
            args: &[],
            culture: self.engine.culture(),
            // A `{:L(x):…}` inside the isolated render switches *its* culture,
            // not the outer call's, exactly as .NET's separate `FormatDetails`
            // does.
            culture_override: Cell::new(None),
            base: self.engine.base,
            // Not part of `FormatDetails` in .NET at all: the collection index
            // lives in a `static`, so an isolated render inside a list still
            // sees the item's index and `{0:list:{:L:{Index}}|,}` looks up
            // `0` and `1` (probed).
            collection_index: Cell::new(self.engine.collection_index.get()),
        };
        let mut output = String::new();
        engine.write_format(format, &[value], self.alignment, &mut output)?;
        Ok(output)
    }

    /// Renders `format` with `pushed` appended to this placeholder's scope
    /// chain — the shared body of the two `format_as_child` calls above.
    ///
    /// The chain is built on the stack while it fits in [`INLINE_SCOPES`].
    /// `ListFormatter` renders a child per item *and* one per spacer, so a
    /// heap-allocated chain would cost `2n` vectors for an `n`-item list, and
    /// a list of lists would multiply that by the inner length. .NET copies
    /// nothing at all: its `FormattingInfo`s are pooled and the chain is a
    /// walk up parent pointers. A stack buffer is the same cost for the depths
    /// a template reaches, and the `Vec` stays as the fallback for the rest.
    fn write_child<'v>(
        &mut self,
        format: &Format,
        pushed: &[&'v Value],
        alignment: i32,
    ) -> Result<(), Error>
    where
        'a: 'v,
    {
        let carried = self.scopes.len();
        let total = carried + pushed.len();
        if total <= INLINE_SCOPES {
            let mut scopes = [&NULL; INLINE_SCOPES];
            scopes[..carried].copy_from_slice(self.scopes);
            scopes[carried..total].copy_from_slice(pushed);
            return self
                .engine
                .write_format(format, &scopes[..total], alignment, self.output);
        }
        let mut scopes = Vec::with_capacity(total);
        scopes.extend_from_slice(self.scopes);
        scopes.extend_from_slice(pushed);
        self.engine
            .write_format(format, &scopes, alignment, self.output)
    }

    /// The value of the outermost scope, which is the argument the format call
    /// was made against (.NET walks its `FormattingInfo` parents up to the root
    /// and takes its `CurrentValue`).
    ///
    /// The `list` formatter renders its spacers against it, so that
    /// `{Names:list:{}|{Split}}` finds `Split` on the object the call was made
    /// with rather than on the list item being formatted.
    pub fn root_value(&self) -> &'a Value {
        self.scopes.first().copied().unwrap_or(&NULL)
    }
}

// ---------------------------------------------------------------------------
// DefaultFormatter
// ---------------------------------------------------------------------------

/// The formatter of last resort, ported from
/// `src/SmartFormat/Extensions/DefaultFormatter.cs`.
///
/// It renders the value with the .NET standard format specifiers, or, when the
/// format contains nested placeholders, evaluates those against the value
/// instead.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFormatter;

impl Formatter for DefaultFormatter {
    /// Itself, so that a caller who registered it can find it again through
    /// [`FormatterRegistry::get_mut`](crate::formatter::FormatterRegistry::get_mut)
    /// and set its knobs.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn name(&self) -> &str {
        "d"
    }

    fn is_default_formatter(&self) -> bool {
        true
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let format = info.format();
        let current = info.current();

        if let Some(format) = format {
            if format.has_nested() {
                info.format_as_child(format, current)?;
                return Ok(true);
            }
        }

        // .NET hands `ISpanFormattable` values the *raw* source text of the
        // format as the specifier, not the escape-resolved `Format.ToString()`.
        let spec = format.map(|format| format.raw.as_str()).unwrap_or_default();
        // .NET reports the index in UTF-16 code units, as everywhere else.
        // Counting them walks the whole template ahead of the placeholder, and
        // only an error ever reads the number, so it is counted lazily.
        let position = || info.engine.utf16_position(info.error_position());
        // Borrowed wherever the text already exists, so the common
        // string / null / bool cases allocate nothing per placeholder.
        let text: Cow<'_, str> = match current {
            Value::Null => Cow::Borrowed(""),
            // .NET bool is not IFormattable, so the spec is ignored.
            Value::Bool(true) => Cow::Borrowed("True"),
            Value::Bool(false) => Cow::Borrowed("False"),
            Value::Int(v) => Cow::Owned(spec_result(
                number::format_number(Number::Int(*v), spec, info.culture()),
                position,
                number::INVALID_SPEC_MESSAGE,
            )?),
            Value::UInt(v) => Cow::Owned(spec_result(
                number::format_number(Number::UInt(*v), spec, info.culture()),
                position,
                number::INVALID_SPEC_MESSAGE,
            )?),
            Value::Float(v) => Cow::Owned(spec_result(
                number::format_number(Number::Float(*v), spec, info.culture()),
                position,
                number::INVALID_SPEC_MESSAGE,
            )?),
            // .NET string is not IFormattable either: `{0:D5}` on a string
            // writes the string unchanged.
            Value::String(v) => Cow::Borrowed(v.as_str()),
            #[cfg(feature = "time")]
            Value::DateTime(v) => Cow::Owned(spec_result(
                date::format_datetime(v, spec, info.culture()),
                position,
                date::INVALID_SPEC_MESSAGE,
            )?),
            // .NET `TimeSpan` is `ISpanFormattable` too, with its own standard
            // specifiers: `c` (and its aliases `t` and `T`), `g` and `G`.
            #[cfg(feature = "time")]
            Value::TimeSpan(v) => Cow::Owned(spec_result(
                crate::extensions::time::format_timespan(v, spec, info.culture()),
                position,
                crate::extensions::time::INVALID_SPEC_MESSAGE,
            )?),
            // A deliberate divergence: .NET falls back to `object.ToString()`
            // and renders the CLR type name (`System.Object[]`,
            // `System.Collections.Generic.Dictionary`2[...]`), which is never
            // what a template author wanted. We fail loudly instead. See
            // DESIGN.md, "Known divergences".
            Value::List(_) => {
                return Err(Error::Format {
                    message: "Default formatting of a list is not supported; use a formatter such as \"list\"".to_owned(),
                    position: position(),
                })
            }
            Value::Map(_) => {
                return Err(Error::Format {
                    message: "Default formatting of a map is not supported; select a value from it"
                        .to_owned(),
                    position: position(),
                })
            }
        };

        info.write(&text);
        Ok(true)
    }
}

/// Turns a spec failure into an [`Error`]. A spec that is not valid .NET at
/// all carries .NET's own `FormatException` message, which is what
/// [`ErrorAction::OutputErrorInResult`] writes; a spec that is valid .NET but
/// outside our subset carries ours, since .NET has no error to mirror there.
///
/// `position` is a closure because it is only ever read on an error path, and
/// counting UTF-16 code units walks the template.
fn spec_result(
    result: Result<String, FormatSpecError>,
    position: impl FnOnce() -> usize,
    invalid_message: &str,
) -> Result<String, Error> {
    result.map_err(|error| match error {
        FormatSpecError::Unsupported(spec) => Error::UnsupportedSpec {
            message: format!("unsupported format spec: {spec}"),
            spec,
            position: position(),
        },
        FormatSpecError::Invalid(_) => Error::Format {
            message: invalid_message.to_owned(),
            position: position(),
        },
    })
}
