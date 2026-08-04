//! Port of SmartFormat.NET's `LocalizationFormatter` (milestone M4).
//!
//! Ported from `src/SmartFormat/Extensions/LocalizationFormatter.cs`,
//! `Utilities/ILocalizationProvider.cs` and `Utilities/LocalizationProvider.cs`.
//!
//! `{:L:Hello}` looks `Hello` up in a [`LocalizationProvider`] and renders what
//! comes back *as a template*: the localized string is parsed and evaluated
//! against the current scope, so `{:L(es):has {:N0} inhabitants}` can be
//! translated to `tiene {:N0} habitantes` and still format the number.
//!
//! .NET's `CreateDefaultSmartFormat` does not register this formatter (nor does
//! [`FormatterRegistry::new`](crate::formatter::FormatterRegistry::new)), so it
//! is always added by hand:
//!
//! ```
//! use smartformat::{HashMapLocalizationProvider, SmartFormatter, Value};
//!
//! let provider: HashMapLocalizationProvider = [
//!     ("", "WeTranslateText", "We translate text"),
//!     ("es", "WeTranslateText", "Traducimos el texto"),
//! ]
//! .into_iter()
//! .collect();
//!
//! let mut smart = SmartFormatter::default();
//! // Builds the formatter from this formatter's parser and settings, and
//! // slots it at .NET's rank for it.
//! smart.register_localization(Box::new(provider));
//!
//! let none = Value::Null;
//! assert_eq!(smart.format("{:L:WeTranslateText}", &none).unwrap(), "We translate text");
//! assert_eq!(
//!     smart.format("{:L(es):WeTranslateText}", &none).unwrap(),
//!     "Traducimos el texto"
//! );
//! ```
//!
//! # The culture the options name is not local
//!
//! .NET assigns `formattingInfo.FormatDetails.Provider = cultureInfo`, and
//! `FormatDetails` belongs to the whole format call, so `{:L(de):…}` switches
//! the culture for *every later placeholder of the template* and not only for
//! the placeholders of the translation. That is reproduced here through
//! [`FormattingInfo::set_culture`], leak and all — probed against 3.6.1:
//! `Format(en-US, "{0:N2}|{1:L(de):WeTranslateText}|{0:N2}", 1234.5, "")` is
//! `1,234.50|Wir übersetzen Text|1.234,50`, and the trailing `{0:N2}` stays
//! German even when the lookup fails and the error is ignored. The switch
//! happens after the empty-format and unknown-culture checks, so neither of
//! those leaks, and it dies with the format call.
//!
//! # Divergences from 3.6.1
//!
//! One, in the isolated render that turns `{:L:{ProductType}}` into a lookup
//! key: .NET builds that render's `FormatDetails` with a `null` provider, which
//! sends every culture-sensitive placeholder inside it to the *ambient*
//! `CultureInfo.CurrentCulture` — so the key `{:L(de):{Num:N2}}` looks up
//! depends on the machine the call runs on. There is no ambient culture here,
//! so [`FormattingInfo::format_to_isolated_string`] renders with the culture in
//! force, which is the one the options just chose. A key built out of a number,
//! a date or anything else culture-sensitive therefore differs; a key built out
//! of strings — every use this formatter was designed for — does not. See
//! `DESIGN.md`, "Known divergences".
//!
//! Everything else — the name, the culture resolution order, the lookup key and
//! its nested-placeholder retry, the parse cache and its case-sensitivity
//! collisions, and every error — is ported and probed against the 3.6.1
//! package.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};

use crate::fmt::culture::{self, CultureData};
use crate::formatter::{Formatter, FormattingInfo};
use crate::parsing::{Format, Parser};
use crate::settings::CaseSensitivity;
use crate::Error;

/// The default formatter name, .NET `LocalizationFormatter.Name`.
///
/// .NET 3.6.1 still carries an obsolete `Names` array holding `localize` and
/// `L`, but only `Name` selects a formatter, so `{:localize:x}` is "No suitable
/// Formatter could be found" there and here (probed).
const NAME: &str = "L";

/// .NET's `ArgumentException` for `{:L:}`, whose format is empty.
const EMPTY_FORMAT: &str =
    "'Format' for localization must not be null or empty. (Parameter 'formattingInfo')";

// ---------------------------------------------------------------------------
// The provider
// ---------------------------------------------------------------------------

/// Resolves a string to its localized equivalent, mirroring .NET
/// `ILocalizationProvider`.
///
/// .NET declares three `GetString` overloads — by ambient UI culture, by
/// culture name, and by `CultureInfo` — but [`LocalizationFormatter`] only ever
/// calls the `CultureInfo` one (probed: every lookup, whatever the template
/// looks like, arrives there), so that is the only one a port needs. The other
/// two are C# conveniences, and the ambient-culture overload has no meaning
/// here at all: this crate has no `CultureInfo.CurrentUICulture` to read, a
/// format call always carries its culture.
///
/// The culture chain is entirely the provider's business. .NET's own
/// `ILocalizationProvider` implementations delegate to a `ResourceManager`,
/// which walks specific culture → neutral parent → invariant, and
/// [`HashMapLocalizationProvider`] walks the same chain.
///
/// `Send + Sync` because a [`Formatter`] is: one registered formatter serves
/// every thread that formats with it.
pub trait LocalizationProvider: Send + Sync {
    /// The localized equivalent of `name` in `culture`, or `None` when there is
    /// none — which the formatter turns into "No localized string found for
    /// '…'".
    ///
    /// Returning `Cow::Borrowed` out of a table owned by the provider is the
    /// cheap path and costs no allocation per placeholder; .NET's `string?`
    /// return is a reference too.
    fn get_string(&self, name: &str, culture: &CultureData) -> Option<Cow<'_, str>>;
}

/// The built-in [`LocalizationProvider`]: per-culture tables held in memory.
///
/// This is the port's stand-in for .NET's `LocalizationProvider`, which wraps
/// `System.Resources.ResourceManager` and reads `.resx` satellite assemblies.
/// There is no `.resx` here (see `DESIGN.md`), so the tables are filled by the
/// caller — from a `HashMap`, from JSON, from anywhere — and the two
/// `ResourceManager` behaviors that are observable through the formatter are
/// reproduced by hand:
///
/// * the **culture chain**: `es-MX` → `es` → the invariant culture `""`, the
///   chain of .NET's `CultureInfo.Parent` ([`culture::parent_name`]), so an
///   entry only present in the invariant table answers a request for any
///   culture — and `zh-CN` reaches a translation filed under `zh-Hans`, which
///   is a step no rule about subtags would take;
/// * the **fallback culture** ([`with_fallback_culture`](Self::with_fallback_culture)),
///   a second chain walked when the first one comes up empty
///   (.NET `LocalizationProvider.FallbackCulture`);
/// * [`return_name_if_not_found`](Self::with_return_name_if_not_found), which
///   answers a miss with the key itself, the way
///   `Microsoft.Extensions.Localization` does.
///
/// Culture names are canonicalized through [`culture::get`] as they are
/// inserted, so `"EN-us"`, `"en-US"` and `"en-us"` all fill one table. A name
/// no shipped culture matches is kept verbatim and can then never be found, as
/// the formatter only ever asks with a [`CultureData`] this crate ships —
/// [`contains_culture`](Self::contains_culture) is there to catch the typo.
///
/// Resource names are matched case-sensitively, as .NET's default
/// `LocalizationProvider(isCaseSensitive: true, …)` matches them. .NET can turn
/// that off, but only because `ResourceManager.IgnoreCase` exists; a caller who
/// needs it here implements [`LocalizationProvider`] instead, which is what the
/// trait is for.
#[derive(Debug, Clone, Default)]
pub struct HashMapLocalizationProvider {
    /// Culture name (canonicalized) → resource name → localized string.
    tables: HashMap<String, HashMap<String, String>>,
    fallback_culture: Option<&'static CultureData>,
    return_name_if_not_found: bool,
}

impl HashMapLocalizationProvider {
    /// An empty provider, which answers every lookup with `None`.
    pub fn new() -> Self {
        Self::default()
    }

    /// A provider filled from `(culture, name, value)` triples, the same thing
    /// [`FromIterator`] does — `HashMapLocalizationProvider::from_triples([("es",
    /// "Yes", "Sí")])` and `[("es", "Yes", "Sí")].into_iter().collect()` are
    /// one call.
    ///
    /// The culture of a triple is a culture *name*: `""` for the invariant
    /// culture, `"es"` for a language, `"es-MX"` for a specific culture.
    pub fn from_triples<I, C, K, V>(triples: I) -> Self
    where
        I: IntoIterator<Item = (C, K, V)>,
        C: AsRef<str>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut provider = Self::new();
        for (culture, name, value) in triples {
            provider.insert(culture.as_ref(), name, value);
        }
        provider
    }

    /// Adds a localized string, returning what was registered under that
    /// culture and name before, if anything.
    pub fn insert(
        &mut self,
        culture: &str,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Option<String> {
        self.tables
            .entry(canonical_culture_name(culture))
            .or_default()
            .insert(name.into(), value.into())
    }

    /// Removes every table (.NET `LocalizationProvider.Clear`).
    pub fn clear(&mut self) {
        self.tables.clear();
    }

    /// Whether a table was filled for that culture — after the same
    /// canonicalization [`insert`](Self::insert) applies, so
    /// `contains_culture("EN-us")` is `contains_culture("en-US")`.
    pub fn contains_culture(&self, culture: &str) -> bool {
        self.tables.contains_key(&canonical_culture_name(culture))
    }

    /// The string registered for exactly that culture, with no chain walked and
    /// no fallback applied.
    pub fn get(&self, culture: &str, name: &str) -> Option<&str> {
        self.table(&canonical_culture_name(culture), name)
    }

    /// The culture consulted when the requested one and its parents have no
    /// entry (.NET `LocalizationProvider.FallbackCulture`, unset by default).
    ///
    /// The fallback is walked as a chain of its own, so a fallback of `fr-FR`
    /// also reaches `fr` and the invariant culture.
    pub fn with_fallback_culture(mut self, culture: &'static CultureData) -> Self {
        self.fallback_culture = Some(culture);
        self
    }

    /// Answers a miss with the requested name instead of `None`
    /// (.NET `LocalizationProvider.ReturnNameIfNotFound`, `false` by default).
    ///
    /// The formatter then never reports "No localized string found": it renders
    /// the key as a template, which is what
    /// `Microsoft.Extensions.Localization.ResourceManagerStringLocalizer` does.
    pub fn with_return_name_if_not_found(mut self, return_name: bool) -> Self {
        self.return_name_if_not_found = return_name;
        self
    }

    /// How many cultures have a table.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    fn table(&self, culture_name: &str, name: &str) -> Option<&str> {
        self.tables.get(culture_name)?.get(name).map(String::as_str)
    }

    /// The .NET `ResourceManager` chain: the culture itself, then each parent
    /// down to the invariant culture.
    fn walk_chain(&self, culture_name: &str, name: &str) -> Option<&str> {
        let mut current = culture_name;
        loop {
            if let Some(value) = self.table(current, name) {
                return Some(value);
            }
            if current.is_empty() {
                return None;
            }
            current = culture::parent_name(current);
        }
    }
}

impl LocalizationProvider for HashMapLocalizationProvider {
    fn get_string(&self, name: &str, culture: &CultureData) -> Option<Cow<'_, str>> {
        if let Some(value) = self.walk_chain(culture.name, name) {
            return Some(Cow::Borrowed(value));
        }
        // .NET tries the fallback culture only after the whole first chain
        // came up empty (`LocalizationProvider.GetString`).
        if let Some(fallback) = self.fallback_culture {
            if let Some(value) = self.walk_chain(fallback.name, name) {
                return Some(Cow::Borrowed(value));
            }
        }
        if self.return_name_if_not_found {
            return Some(Cow::Owned(name.to_owned()));
        }
        None
    }
}

impl<C, K, V> FromIterator<(C, K, V)> for HashMapLocalizationProvider
where
    C: AsRef<str>,
    K: Into<String>,
    V: Into<String>,
{
    fn from_iter<I: IntoIterator<Item = (C, K, V)>>(triples: I) -> Self {
        Self::from_triples(triples)
    }
}

/// The name a culture is filed under: the shipped culture's own spelling when
/// this crate knows the name — `"EN-us"` and `"en_US"` are `"en-US"` and
/// `"en"` respectively, as .NET's `GetCultureInfo` resolves them — and the name
/// as written when it does not.
fn canonical_culture_name(culture: &str) -> String {
    culture::get(culture).map_or_else(|| culture.to_owned(), |data| data.name.to_owned())
}

// ---------------------------------------------------------------------------
// The formatter
// ---------------------------------------------------------------------------

/// Renders a localized string as a template, ported from .NET
/// `LocalizationFormatter`.
///
/// The format's raw text is the lookup key — `{:L:Hello}` looks up `Hello` —
/// and what the [`LocalizationProvider`] returns is *parsed and evaluated*
/// against the current scope rather than written out, so a translation can
/// carry placeholders of its own. Parsing happens once per distinct localized
/// string and is cached, as .NET caches it in `LocalizedFormatCache`.
///
/// The culture is picked in .NET's order:
///
/// 1. the formatter options, `{:L(fr):…}`, trimmed — .NET
///    `CultureInfo.GetCultureInfo(culture)`;
/// 2. the culture of the format call, `{:L:…}` or `{:L():…}` — .NET's
///    `FormatDetails.Provider`, when it is a `CultureInfo`;
/// 3. .NET then falls back to `CultureInfo.CurrentUICulture`. There is no such
///    thing here: a format call always carries a culture, and
///    [`SmartFormatter::format`](crate::SmartFormatter::format) makes it the
///    invariant one. Step 3 is therefore unreachable rather than absent.
///
/// A placeholder always has to name the formatter: .NET's `CanAutoDetect`
/// throws when it is set to `true`, so [`can_auto_detect`](Formatter::can_auto_detect)
/// is `false` and cannot be changed.
pub struct LocalizationFormatter {
    name: String,
    /// The parser localized strings are parsed with — .NET reaches the owning
    /// `SmartFormatter`'s through `IInitializer.Initialize`; a [`Formatter`]
    /// here cannot hold that reference, so it gets a parser of its own, built
    /// from the host's [`ParserSettings`](crate::parsing::ParserSettings).
    parser: Parser,
    provider: Box<dyn LocalizationProvider>,
    /// How the [`cache`](Self::cache) is keyed, .NET's
    /// `SmartSettings.GetCaseSensitivityComparer()`.
    case_sensitivity: CaseSensitivity,
    /// .NET `LocalizedFormatCache`: localized string → the parsed format, so a
    /// string is parsed once however often it is rendered.
    ///
    /// `Arc` rather than a borrow out of the lock: a localized string may hold
    /// a `{:L:…}` of its own, and rendering it while a read guard was held
    /// would deadlock against any writer. The clone is one refcount bump.
    cache: RwLock<HashMap<String, Arc<Format>>>,
}

impl LocalizationFormatter {
    /// A formatter named `L` that resolves strings through `provider`.
    ///
    /// `parser` is the parser of the [`SmartFormatter`](crate::SmartFormatter)
    /// that will render the localized strings — .NET reaches it through
    /// `_formatter.Parser`, which `Initialize` hands it. Its *settings* are
    /// copied, so a localized string is parsed exactly as the template that
    /// asked for it was.
    ///
    /// Unlike [`TemplateFormatter::new`](crate::extensions::TemplateFormatter::new)
    /// this does not need the host's [`CaseSensitivity`]: it decides only how
    /// the parse cache is keyed, and the default —
    /// [`CaseSensitivity::CaseSensitive`], .NET's own default — is the one that
    /// never confuses two localized strings. See
    /// [`with_case_sensitivity`](Self::with_case_sensitivity).
    pub fn new(parser: &Parser, provider: Box<dyn LocalizationProvider>) -> Self {
        Self {
            name: NAME.to_owned(),
            parser: Parser::new(parser.settings().clone()),
            provider,
            case_sensitivity: CaseSensitivity::CaseSensitive,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// How the parse cache is keyed — pass the host's
    /// [`SmartSettings::case_sensitive`](crate::SmartSettings::case_sensitive)
    /// to match .NET exactly.
    ///
    /// This is observable, and the observation is a .NET bug worth reproducing:
    /// the cache is keyed by the *localized string* with the host's comparer,
    /// so under [`CaseSensitivity::CaseInsensitive`] two translations that
    /// differ only in case collide and the first one parsed wins. Probed
    /// against 3.6.1: with translations `abc {0}` and `ABC {0}`, the second
    /// renders as `abc x`.
    pub fn with_case_sensitivity(mut self, case_sensitivity: CaseSensitivity) -> Self {
        self.case_sensitivity = case_sensitivity;
        self
    }

    /// The provider strings are resolved through.
    pub fn provider(&self) -> &dyn LocalizationProvider {
        self.provider.as_ref()
    }

    /// Swaps the provider out. The parse cache is emptied with it: it holds
    /// formats parsed from the old provider's strings.
    pub fn set_provider(&mut self, provider: Box<dyn LocalizationProvider>) {
        self.provider = provider;
        self.clear_cache();
    }

    /// How the parse cache is keyed. See
    /// [`with_case_sensitivity`](Self::with_case_sensitivity).
    pub fn case_sensitivity(&self) -> CaseSensitivity {
        self.case_sensitivity
    }

    /// How many distinct localized strings have been parsed and cached
    /// (.NET `LocalizedFormatCache.Count`).
    pub fn cached_len(&self) -> usize {
        self.cache().len()
    }

    /// Empties the parse cache.
    pub fn clear_cache(&mut self) {
        self.cache_mut().clear();
    }

    /// The culture .NET picks in `GetCultureInfo`: the trimmed formatter
    /// options, or `None` when there are none.
    ///
    /// `None` is "leave the culture alone". .NET answers that case with
    /// `FormatDetails.Provider` itself, which the assignment that follows puts
    /// straight back, so there is nothing to switch to.
    ///
    /// .NET turns a name `CultureInfo.GetCultureInfo` rejects into a
    /// `LocalizationFormattingException` reported at index 0. This crate ships
    /// data for a fixed list of cultures instead of resolving any well-formed
    /// name against CLDR, so the *set* of names that fail is wider here — see
    /// `DESIGN.md` — but the failure is shaped the same way.
    fn culture(&self, info: &FormattingInfo<'_>) -> Result<Option<&'static CultureData>, Error> {
        // .NET reads `FormatterOptions`, which resolves escape sequences and
        // throws when one resolves to nothing — outside the evaluator's error
        // handling, so such a placeholder fails the call whatever the error
        // action is.
        let name = info.formatter_options()?.trim();
        if name.is_empty() {
            return Ok(None);
        }
        culture::get(name).map(Some).ok_or_else(|| {
            info.formatting_error(
                &format!("unknown culture {name:?}: no data is shipped for it"),
                0,
            )
        })
    }

    /// The parsed form of a localized string, from the cache or parsed into it
    /// (.NET's `LocalizedFormatCache` lookup plus `Parser.ParseFormat`).
    ///
    /// A localized string that does not parse fails here, as .NET's
    /// `ParseFormat` throws its `ParsingErrors` straight through
    /// `TryEvaluateFormat`.
    fn parsed(&self, localized: &str) -> Result<Arc<Format>, Error> {
        // A hit is the common case, and the case-sensitive key is the string
        // itself, so the lookup borrows and only an insert owns.
        let key = self.cache_key(localized);
        if let Some(cached) = self.cache().get(key.as_ref()) {
            return Ok(Arc::clone(cached));
        }
        let parsed = Arc::new(self.parser.parse(localized)?);
        // Another thread may have parsed the same string in the meantime; its
        // format renders the same, so whichever landed first stays.
        Ok(Arc::clone(
            self.cache_mut().entry(key.into_owned()).or_insert(parsed),
        ))
    }

    /// The cache key of a localized string: the string itself, or its
    /// uppercased form when names are matched case-insensitively, which is what
    /// makes two translations that differ only in case collide as they do in
    /// .NET.
    ///
    /// .NET folds with `StringComparer.OrdinalIgnoreCase`, whose *simple*
    /// mapping leaves `ß` alone where `to_uppercase` writes `SS`; two
    /// translations that differ only in that collide here and not there.
    fn cache_key<'k>(&self, localized: &'k str) -> Cow<'k, str> {
        match self.case_sensitivity {
            CaseSensitivity::CaseSensitive => Cow::Borrowed(localized),
            CaseSensitivity::CaseInsensitive => Cow::Owned(localized.to_uppercase()),
        }
    }

    fn cache(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, Arc<Format>>> {
        // A poisoned cache is still a valid cache: nothing that touches it can
        // leave it half-written, and a formatter that stopped memoizing after
        // an unrelated panic would be a worse failure than a stale entry.
        self.cache.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn cache_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, Arc<Format>>> {
        self.cache.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Formatter for LocalizationFormatter {
    /// Itself, which is what lets
    /// [`SmartFormatter::register_localization`](crate::SmartFormatter::register_localization)
    /// find this formatter again and replace its provider, the way .NET's one
    /// `SmartSettings.Localization.LocalizationProvider` slot behaves.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn name(&self) -> &str {
        &self.name
    }

    /// Never: .NET's `CanAutoDetect` setter throws for `true`
    /// ("LocalizationFormatter cannot handle auto-detection").
    fn can_auto_detect(&self) -> bool {
        false
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        // .NET checks the format first, before it looks at the provider or the
        // culture, so `{:L(bad!!!):}` reports the empty format (probed).
        // `{:L:}` and `{:L()}` both carry a format of length zero.
        let format = match info.format() {
            Some(format) if !format.raw.is_empty() => format,
            // A `LocalizationFormattingException`, so the message carries the
            // full envelope — and .NET reports it at index 0, not at the
            // placeholder.
            _ => return Err(info.formatting_error(EMPTY_FORMAT, 0)),
        };

        // .NET's next check is `LocalizationProvider is null`, a
        // `NullReferenceException` its `Initialize` can leave behind when the
        // settings carry no provider. A provider is not optional here, so
        // there is nothing to check.

        // .NET `formattingInfo.FormatDetails.Provider = cultureInfo`, which
        // switches the culture for the rest of the format call: the
        // placeholders of the localized string, and every later placeholder of
        // the template as well. It happens before the lookup, so it stands even
        // when the lookup then fails — but after the two checks above, so
        // `{:L(bad!!!):x}` and `{:L(de):}` leave the culture alone (probed).
        if let Some(chosen) = self.culture(info)? {
            info.set_culture(chosen);
        }
        let culture = info.culture();

        let key = raw_text(info, format)?;
        let mut localized = self.provider.get_string(&key, culture);

        // When the raw text misses and the format has nested placeholders,
        // .NET renders the format into a scratch buffer — against the current
        // value, with no enclosing scopes and no positional arguments — and
        // looks the *result* up instead, which is how `{:L:{ProductType}}`
        // finds the entry for `paper`.
        if localized.is_none() && format.has_nested() {
            let rendered = info.format_to_isolated_string(format, info.current())?;
            localized = self.provider.get_string(&rendered, culture);
        }

        let Some(localized) = localized else {
            // A `LocalizationFormattingException` again, envelope and all,
            // reported at the start of the format. The key quoted is the raw
            // text even when the nested-key lookup above was the one that
            // missed.
            return Err(
                info.formatting_error_here(&format!("No localized string found for '{key}'"))
            );
        };

        let localized = self.parsed(&localized)?;
        // .NET `FormatAsChild(format, CurrentValue)`: the localized string is
        // rendered with the current value pushed as its scope, into the same
        // output and with the placeholder's alignment.
        let value = info.current();
        info.format_as_child(&localized, value)?;
        Ok(true)
    }
}

impl fmt::Debug for LocalizationFormatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalizationFormatter")
            .field("name", &self.name)
            .field("case_sensitivity", &self.case_sensitivity)
            .field("cached", &self.cached_len())
            .finish_non_exhaustive()
    }
}

/// .NET `Format.RawText`, the lookup key: the literal text with escape
/// sequences resolved, plus the *unresolved* source text of every placeholder,
/// so `{:L:a\{b}` looks up `a{b` and `{:L:{ProductType}}` looks up
/// `{ProductType}` (both probed).
///
/// A sequence that resolves to nothing is left as written by the parser and
/// reported by `LiteralText::escape_error`, which .NET raises as an
/// `ArgumentException` while `Format.ToString()` concatenates the items — so
/// the first failing literal decides, and the message carries no envelope.
fn raw_text(info: &FormattingInfo<'_>, format: &Format) -> Result<String, Error> {
    if let Some((message, start)) = format.first_escape_error() {
        return Err(info.plain_error(message, start));
    }
    // `Display` writes exactly what .NET concatenates: `LiteralText.AsSpan()`
    // is escape-resolved, `Placeholder.AsSpan()` is the source text.
    Ok(format.to_string())
}

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/LocalizationFormatterTests.cs` and
    //! `LocalizationProviderTests.cs`, plus the cases probed against the pinned
    //! SmartFormat.NET 3.6.1 package.
    //!
    //! The .NET fixture reads `.resx` resources (`src/SmartFormat.Tests/
    //! Localization/LocTest1.*.resx`); there are none here, so the same
    //! translations are put into a [`HashMapLocalizationProvider`] by hand.
    //! `LocalizationFormatter` is not part of .NET's `CreateDefaultSmartFormat`
    //! either, so every test registers it, as the .NET fixture does with
    //! `AddExtensions`.

    use std::collections::BTreeMap;

    use super::*;
    use crate::formatter::FormatterRegistry;
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::value::Value;
    use crate::SmartFormatter;

    /// The translations of the .NET fixture's `LocTest1` resources, as
    /// `(culture, name, value)` triples. The empty culture is `LocTest1.resx`
    /// itself, which .NET compiles into the invariant (neutral) resource.
    fn translations() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("", "WeTranslateText", "We translate text"),
            ("es", "WeTranslateText", "Traducimos el texto"),
            ("fr", "WeTranslateText", "Nous traduisons des textes"),
            ("de", "WeTranslateText", "Wir übersetzen Text"),
            (
                "",
                "OnlyExistForInvariantCulture",
                "This entry only exists in the invariant culture resource",
            ),
            // The .NET fixture writes the number as `{:#,#}`, a custom numeric
            // format this port does not support (see `DESIGN.md`); `N0` renders
            // the same digits for these values and keeps the point of the case,
            // which is that the number follows the localized culture.
            (
                "",
                "{0} has {1:N0} inhabitants",
                "{0} has {1:N0} inhabitants",
            ),
            (
                "es",
                "{0} has {1:N0} inhabitants",
                "{0} tiene {1:N0} habitantes",
            ),
            ("", "has {:N0} inhabitants", "has {:N0} inhabitants"),
            ("es", "has {:N0} inhabitants", "tiene {:N0} habitantes"),
            ("fr", "has {:N0} inhabitants", "compte {:N0} habitants"),
            ("de", "has {:N0} inhabitants", "hat {:N0} Einwohner"),
            ("", "{} item", "{} item"),
            ("", "{} items", "{} items"),
            ("es", "{} item", "{} elemento"),
            ("es", "{} items", "{} elementos"),
            ("fr", "{} item", "{} élément"),
            ("fr", "{} items", "{} éléments"),
            ("de", "{} item", "{} Element"),
            ("de", "{} items", "{} Elemente"),
            ("", "paper", "Paper"),
            ("de", "paper", "das Papier"),
            ("fr", "paper", "Papier"),
            ("", "pen", "Pen"),
            ("de", "pen", "der Kugelschreiber"),
            ("fr", "pen", "Bic"),
        ]
    }

    fn provider() -> HashMapLocalizationProvider {
        translations().into_iter().collect()
    }

    /// A formatter with the fixture's translations registered, and nothing but
    /// the formatter under test and the default formatter, so these tests pin
    /// this formatter rather than the order of the default registry.
    fn smart_with(provider: impl LocalizationProvider + 'static) -> SmartFormatter {
        smart_with_settings(provider, SmartSettings::default())
    }

    fn smart_with_settings(
        provider: impl LocalizationProvider + 'static,
        settings: SmartSettings,
    ) -> SmartFormatter {
        let case_sensitivity = settings.case_sensitive;
        let mut smart = SmartFormatter::new(settings);
        let formatter = LocalizationFormatter::new(smart.parser(), Box::new(provider))
            .with_case_sensitivity(case_sensitivity);
        smart.formatters_mut().add(Box::new(formatter));
        smart
    }

    fn smart() -> SmartFormatter {
        smart_with(provider())
    }

    fn none() -> Value {
        Value::Null
    }

    fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn format(smart: &SmartFormatter, template: &str) -> String {
        format_args_culture(smart, template, &none(), "")
    }

    fn format_in(smart: &SmartFormatter, template: &str, culture: &str) -> String {
        format_args_culture(smart, template, &none(), culture)
    }

    fn format_args_culture(
        smart: &SmartFormatter,
        template: &str,
        args: &Value,
        culture: &str,
    ) -> String {
        smart
            .format_with_culture_name(template, args, culture)
            .unwrap_or_else(|error| panic!("{template:?} in {culture:?} failed: {error}"))
    }

    fn error_of(smart: &SmartFormatter, template: &str) -> String {
        match smart.format(template, &none()) {
            Err(Error::Format { message, .. }) => message,
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // The formatter itself
    // -----------------------------------------------------------------------

    #[test]
    fn name_and_defaults_match_dotnet() {
        let smart = SmartFormatter::default();
        let formatter = LocalizationFormatter::new(
            smart.parser(),
            Box::new(HashMapLocalizationProvider::new()),
        );
        assert_eq!(formatter.name(), "L");
        // .NET's `CanAutoDetect` setter throws for `true`, so there is nothing
        // to turn on here.
        assert!(!formatter.can_auto_detect());
        assert_eq!(formatter.case_sensitivity(), CaseSensitivity::CaseSensitive);
        assert_eq!(formatter.cached_len(), 0);
        assert_eq!(formatter.with_name("localize").name(), "localize");
    }

    #[test]
    fn only_the_name_selects_the_formatter() {
        // The obsolete .NET `Names` array holds "localize" as well, but only
        // `Name` is looked up (probed against 3.6.1).
        let smart = smart();
        assert_eq!(format(&smart, "{:L:WeTranslateText}"), "We translate text");
        assert!(error_of(&smart, "{:localize:WeTranslateText}")
            .starts_with("Error parsing format string: No suitable Formatter could be found"));
    }

    #[test]
    fn the_formatter_name_follows_the_case_sensitivity_setting() {
        // `{:l(es):…}` is a different formatter under the default setting and
        // the same one when names are matched case-insensitively (probed).
        let sensitive = smart();
        assert!(error_of(&sensitive, "{:l:WeTranslateText}")
            .starts_with("Error parsing format string: No suitable Formatter could be found"));

        let insensitive = smart_with_settings(
            provider(),
            SmartSettings {
                case_sensitive: CaseSensitivity::CaseInsensitive,
                ..SmartSettings::default()
            },
        );
        assert_eq!(
            format(&insensitive, "{:l:WeTranslateText}"),
            "We translate text"
        );
    }

    #[test]
    fn never_auto_detects() {
        // With no formatter named, the default formatter takes the placeholder
        // and `WeTranslateText` is a format specifier, not a lookup key.
        let smart = smart();
        assert_eq!(format(&smart, "{:WeTranslateText}"), "");
    }

    // -----------------------------------------------------------------------
    // Culture resolution
    // -----------------------------------------------------------------------

    #[test]
    fn pure_text_culture_by_format_string() {
        let smart = smart();
        for (template, expected) in [
            ("{:L(es):WeTranslateText}", "Traducimos el texto"),
            ("{:L(en):WeTranslateText}", "We translate text"),
            ("{:L(fr):WeTranslateText}", "Nous traduisons des textes"),
            ("{:L(de):WeTranslateText}", "Wir übersetzen Text"),
        ] {
            assert_eq!(format(&smart, template), expected, "{template}");
        }
    }

    #[test]
    fn pure_text_culture_by_call() {
        let smart = smart();
        for (template, expected, culture) in [
            ("{:L():WeTranslateText}", "Traducimos el texto", "es"),
            ("{:L:WeTranslateText}", "Traducimos el texto", "es"),
            ("{:L():WeTranslateText}", "We translate text", ""),
            ("{:L:WeTranslateText}", "We translate text", ""),
        ] {
            assert_eq!(format_in(&smart, template, culture), expected, "{template}");
        }
    }

    #[test]
    fn the_options_win_over_the_calls_culture() {
        let smart = smart();
        assert_eq!(
            format_in(&smart, "{:L(de):WeTranslateText}", "fr-FR"),
            "Wir übersetzen Text"
        );
    }

    #[test]
    fn the_options_are_trimmed() {
        // .NET `formattingInfo.FormatterOptions.Trim()`; probed with tabs too.
        let smart = smart();
        for template in [
            "{:L( es ):WeTranslateText}",
            "{:L(\tes):WeTranslateText}",
            "{:L(es\t):WeTranslateText}",
        ] {
            assert_eq!(
                format(&smart, template),
                "Traducimos el texto",
                "{template}"
            );
        }
    }

    #[test]
    fn a_specific_culture_falls_back_to_its_language() {
        // .NET's `ResourceManager` chain, reproduced by the provider: es-MX has
        // no table of its own, so `es` answers.
        let smart = smart();
        assert_eq!(
            format(&smart, "{:L(es-MX):WeTranslateText}"),
            "Traducimos el texto"
        );
    }

    #[test]
    fn no_localized_string_only_in_invariant_culture() {
        // .NET `No_Localized_String_Only_In_Fallback_Culture`: `pt` has no
        // table at all, so the chain reaches the invariant resource.
        let smart = smart();
        assert_eq!(
            format_in(&smart, "{:L:OnlyExistForInvariantCulture}", "pt"),
            "This entry only exists in the invariant culture resource"
        );
    }

    #[test]
    fn a_culture_this_crate_does_not_ship_is_an_error() {
        // .NET `Unknown_Culture_Should_Throw`. .NET rejects only names
        // `CultureInfo.GetCultureInfo` cannot parse — probed: `unknown` and
        // `xx-XX` are perfectly good custom cultures there, `zz-ZZ-really-bad!`
        // is not — while this crate rejects every name it ships no data for.
        // The failure is shaped the same way: index 0, full envelope.
        let smart = smart();
        let message = error_of(&smart, "{:L(zz-ZZ-really-bad!):WeTranslateText}");
        assert!(
            message.starts_with(
                "Error parsing format string: unknown culture \"zz-ZZ-really-bad!\": \
                 no data is shipped for it at 0"
            ),
            "{message}"
        );
    }

    // -----------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------

    #[test]
    fn missing_format_is_an_error() {
        // .NET `Missing_Format_Should_Throw`, an `ArgumentException` wrapped in
        // a `LocalizationFormattingException` reported at index 0.
        let smart = smart();
        for template in ["{:L:}", "{:L()}", "{:L():}"] {
            let message = error_of(&smart, template);
            assert!(
                message.starts_with(&std::format!(
                    "Error parsing format string: {EMPTY_FORMAT} at 0"
                )),
                "{template}: {message}"
            );
        }
    }

    #[test]
    fn no_localized_string_found() {
        // .NET `No_Localized_String_Found`, once per error action.
        let smart = smart();
        let message = error_of(&smart, "{:L(es):NonExisting}");
        assert_eq!(
            message,
            "Error parsing format string: No localized string found for 'NonExisting' at 8\n\
             {:L(es):NonExisting}\n--------^"
        );

        for (action, expected) in [
            (ErrorAction::Ignore, ""),
            (ErrorAction::MaintainTokens, "{:L(es):NonExisting}"),
        ] {
            let smart = smart_with_settings(
                provider(),
                SmartSettings {
                    format_error_action: action,
                    ..SmartSettings::default()
                },
            );
            assert_eq!(
                format(&smart, "{:L(es):NonExisting}"),
                expected,
                "{action:?}"
            );
        }

        let smart = smart_with_settings(
            provider(),
            SmartSettings {
                format_error_action: ErrorAction::OutputErrorInResult,
                ..SmartSettings::default()
            },
        );
        assert!(format(&smart, "{:L(es):NonExisting}").contains("No localized string found"));
    }

    #[test]
    fn no_localized_string_found_with_name_fallback() {
        // .NET `No_Localized_String_Found_With_Name_Fallback`: the provider
        // answers with the key, which is then rendered as a template.
        let smart = smart_with(provider().with_return_name_if_not_found(true));
        assert_eq!(format(&smart, "{:L(es):NonExisting}"), "NonExisting");
    }

    #[test]
    fn an_escape_sequence_that_resolves_to_nothing_fails_the_key() {
        // `Format.RawText` resolves escape sequences, so a bad one throws while
        // the key is built — a bare `ArgumentException`, no envelope (probed).
        let smart = smart();
        assert_eq!(
            error_of(&smart, r"{:L:a\qb}"),
            r#"Unrecognized escape sequence "\q" in literal."#
        );
    }

    #[test]
    fn a_resolved_escape_sequence_is_part_of_the_key() {
        // Probed: `{:L:a\{b}` looks up `a{b`.
        let smart = smart_with(HashMapLocalizationProvider::from_triples([(
            "", "a{b", "escaped",
        )]));
        assert_eq!(format(&smart, r"{:L:a\{b}"), "escaped");
    }

    #[test]
    fn a_localized_string_that_does_not_parse_fails() {
        // .NET's `ParseFormat` throws its `ParsingErrors` straight through the
        // formatter (probed).
        let smart = smart_with(HashMapLocalizationProvider::from_triples([(
            "", "BadParse", "{0:",
        )]));
        match smart.format("{:L:BadParse}", &none()) {
            Err(Error::Parse { errors, .. }) => {
                assert_eq!(errors.len(), 1);
                assert!(
                    errors[0].message.contains("closing brace"),
                    "{}",
                    errors[0].message
                );
            }
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Rendering the localized string
    // -----------------------------------------------------------------------

    #[test]
    fn the_localized_string_is_a_template() {
        // .NET `TextWithPlaceholder_CultureByFormatString`, the "best practice"
        // half: the selector stays outside the localized text, so one
        // translation serves every selector.
        //
        // The .NET case picks the culture in the formatter options; this one
        // takes it from the call, so that the number and the translation are
        // chosen by the same thing without the leak the options carry —
        // `the_options_culture_reaches_the_localized_placeholders` covers that.
        let args = Value::List(vec![Value::from("X-City"), Value::from(8_900_000i64)]);
        let smart = smart();
        assert_eq!(
            format_args_culture(&smart, "{0} {1:L:has {:N0} inhabitants}", &args, "de"),
            "X-City hat 8.900.000 Einwohner"
        );
        assert_eq!(
            format_args_culture(&smart, "{0} {1:L:has {:N0} inhabitants}", &args, "fr"),
            "X-City compte 8\u{202f}900\u{202f}000 habitants"
        );
        assert_eq!(
            format_args_culture(&smart, "{0} {1:L:has {:N0} inhabitants}", &args, ""),
            "X-City has 8,900,000 inhabitants"
        );
    }

    #[test]
    fn the_localized_string_sees_the_positional_arguments() {
        // .NET's "possible, but not recommended" cases: the selectors are part
        // of the localized text, which `FormatAsChild` renders with the
        // arguments of the format call in scope.
        let args = Value::List(vec![Value::from("X-City"), Value::from(8_900_000i64)]);
        let smart = smart();
        assert_eq!(
            format_args_culture(&smart, "{:L:{0} has {1:N0} inhabitants}", &args, ""),
            "X-City has 8,900,000 inhabitants"
        );
        assert_eq!(
            format_args_culture(&smart, "{:L:{0} has {1:N0} inhabitants}", &args, "es"),
            "X-City tiene 8.900.000 habitantes"
        );
    }

    #[test]
    fn the_localized_string_is_rendered_against_the_current_value() {
        let smart = smart_with(HashMapLocalizationProvider::from_triples([
            ("", "greet", "Hello, {Name}!"),
            ("de", "greet", "Hallo, {Name}!"),
        ]));
        let args = map([("Name", Value::from("Joe"))]);
        assert_eq!(
            format_args_culture(&smart, "{:L(de):greet}", &args, ""),
            "Hallo, Joe!"
        );
    }

    // -----------------------------------------------------------------------
    // The culture the options name, which .NET assigns to the whole call
    // -----------------------------------------------------------------------

    #[test]
    fn the_options_culture_reaches_the_localized_placeholders() {
        // .NET assigns `FormatDetails.Provider = cultureInfo`, so the German
        // translation is written with German number formatting — probed
        // against 3.6.1: `{0} {1:L(de):has {:N0} inhabitants}` from an
        // invariant call is "X-City hat 8.900.000 Einwohner".
        let args = Value::List(vec![Value::from("X-City"), Value::from(8_900_000i64)]);
        let smart = smart();
        assert_eq!(
            format_args_culture(&smart, "{0} {1:L(de):has {:N0} inhabitants}", &args, ""),
            "X-City hat 8.900.000 Einwohner"
        );
        assert_eq!(
            format_args_culture(&smart, "{0} {1:L(es):has {:N0} inhabitants}", &args, ""),
            "X-City tiene 8.900.000 habitantes"
        );
    }

    #[test]
    fn the_options_culture_leaks_into_the_rest_of_the_call() {
        // `FormatDetails` belongs to the format call, so the switch outlives
        // the placeholder that made it — probed against 3.6.1:
        // `Format(en-US, "{0:N2}|{1:L(de):WeTranslateText}|{0:N2}", 1234.5, "")`
        // is "1,234.50|Wir übersetzen Text|1.234,50".
        let smart = smart();
        let args = Value::List(vec![Value::from(1234.5f64), Value::from("")]);
        assert_eq!(
            format_args_culture(
                &smart,
                "{0:N2}|{1:L(de):WeTranslateText}|{0:N2}",
                &args,
                "en-US"
            ),
            "1,234.50|Wir übersetzen Text|1.234,50"
        );

        // A second call on the same formatter starts from its own culture.
        assert_eq!(
            format_args_culture(&smart, "{0:N2}", &args, "en-US"),
            "1,234.50"
        );
    }

    #[test]
    fn the_options_culture_leaks_even_when_the_lookup_fails() {
        // Probed: the assignment happens before the lookup, so an ignored
        // "No localized string found" still leaves German behind.
        let smart = smart_with_settings(
            provider(),
            SmartSettings {
                format_error_action: ErrorAction::Ignore,
                ..SmartSettings::default()
            },
        );
        let args = Value::List(vec![Value::from(1234.5f64), Value::from("")]);
        assert_eq!(
            format_args_culture(
                &smart,
                "{0:N2}|{1:L(de):NonExisting}|{0:N2}",
                &args,
                "en-US"
            ),
            "1,234.50||1.234,50"
        );
    }

    #[test]
    fn a_rejected_culture_or_an_empty_format_leaves_the_culture_alone() {
        // Both checks come before the assignment in .NET, so neither leaks
        // (probed: "1,234.50||1,234.50" for each).
        let smart = smart_with_settings(
            provider(),
            SmartSettings {
                format_error_action: ErrorAction::Ignore,
                ..SmartSettings::default()
            },
        );
        let args = Value::List(vec![Value::from(1234.5f64), Value::from("")]);
        for template in [
            "{0:N2}|{1:L(zz-ZZ-really-bad!):WeTranslateText}|{0:N2}",
            "{0:N2}|{1:L(de):}|{0:N2}",
        ] {
            assert_eq!(
                format_args_culture(&smart, template, &args, "en-US"),
                "1,234.50||1,234.50",
                "{template}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The key that only matches once the nested placeholders are rendered
    // -----------------------------------------------------------------------

    #[test]
    fn a_nested_key_is_rendered_and_looked_up_again() {
        // .NET evaluates a format with nested placeholders and looks the
        // *result* up when the raw text misses — probed against 3.6.1:
        // `{:L:{ProductType}}` with `ProductType = "paper"` is "Paper" from an
        // invariant call and "das Papier" from a German one.
        let smart = smart();
        let args = map([("ProductType", Value::from("paper"))]);
        assert_eq!(
            format_args_culture(&smart, "{:L:{ProductType}}", &args, ""),
            "Paper"
        );
        assert_eq!(
            format_args_culture(&smart, "{:L:{ProductType}}", &args, "de"),
            "das Papier"
        );
        // The options pick the culture of the second lookup too.
        assert_eq!(
            format_args_culture(&smart, "{:L(fr):{ProductType}}", &args, ""),
            "Papier"
        );
        // Literals and several placeholders are all part of the rendered key.
        assert_eq!(
            format_args_culture(
                &smart,
                "{:L:pa{Tail}}",
                &map([("Tail", Value::from("per"))]),
                ""
            ),
            "Paper"
        );
    }

    #[test]
    fn the_isolated_render_has_no_arguments_and_no_enclosing_scope() {
        // Probed: .NET builds a `FormatDetails` with `InitializationObject
        // .ObjectList` for the arguments and a `FormattingInfo` with no parent,
        // so a positional selector and a selector of an enclosing scope both
        // fail — with the *outer* template quoted, and at an offset into it.
        let smart = smart();

        let args = Value::List(vec![Value::from("paper")]);
        let message = match smart.format_with_culture_name("{:L(es):{0}}", &args, "") {
            Err(Error::Format { message, .. }) => message,
            other => panic!("expected a formatting error, got {other:?}"),
        };
        assert_eq!(
            message,
            "Error parsing format string: No source extension could handle the \
             selector named \"0\" at 9\n{:L(es):{0}}\n---------^"
        );

        let args = map([
            ("Outer", Value::from("paper")),
            ("Inner", map([("X", Value::from(1i64))])),
        ]);
        let message = match smart.format_with_culture_name("{Inner:L:{Outer}}", &args, "") {
            Err(Error::Format { message, .. }) => message,
            other => panic!("expected a formatting error, got {other:?}"),
        };
        assert_eq!(
            message,
            "Error parsing format string: No source extension could handle the \
             selector named \"Outer\" at 10\n{Inner:L:{Outer}}\n----------^"
        );

        // The same selector on the current value is found, which is what makes
        // the two above a scope reset rather than a broken lookup.
        let args = map([("Inner", map([("Outer", Value::from("paper"))]))]);
        assert_eq!(
            format_args_culture(&smart, "{Inner:L:{Outer}}", &args, ""),
            "Paper"
        );
    }

    #[test]
    fn the_isolated_render_handles_its_own_errors() {
        // The render runs under the call's error action, so a placeholder it
        // cannot evaluate contributes nothing to the key instead of failing
        // the call — and the miss that follows quotes the *raw text*, never the
        // rendered key (probed under each action).
        for (action, expected) in [
            (ErrorAction::Ignore, ""),
            (ErrorAction::MaintainTokens, "{:L(es):{0}}"),
        ] {
            let smart = smart_with_settings(
                provider(),
                SmartSettings {
                    format_error_action: action,
                    ..SmartSettings::default()
                },
            );
            let args = Value::List(vec![Value::from("paper")]);
            assert_eq!(
                format_args_culture(&smart, "{:L(es):{0}}", &args, ""),
                expected,
                "{action:?}"
            );
        }

        let smart = smart_with_settings(
            provider(),
            SmartSettings {
                format_error_action: ErrorAction::OutputErrorInResult,
                ..SmartSettings::default()
            },
        );
        let args = Value::List(vec![Value::from("paper")]);
        assert!(format_args_culture(&smart, "{:L(es):{0}}", &args, "")
            .contains("No localized string found for '{0}'"));
    }

    #[test]
    fn the_alignment_reaches_the_isolated_render() {
        // .NET takes the isolated `FormattingInfo`'s alignment from
        // `format.ParentPlaceholder`, and a nested placeholder inherits it from
        // the parser, so both the literal and the placeholder of the key are
        // padded — probed: `{,20:L:{ProductType}}` looks up
        // "               paper" and misses.
        let smart = smart_with(HashMapLocalizationProvider::from_triples([
            ("", "               paper", "padded key"),
            ("", "        pa       per", "padded literal and placeholder"),
        ]));
        let args = map([("ProductType", Value::from("paper"))]);
        // The translation is then written with that same alignment, which is
        // the ordinary `{,20:L:…}` padding and not the key's.
        assert_eq!(
            format_args_culture(&smart, "{,20:L:{ProductType}}", &args, ""),
            "          padded key"
        );
        assert_eq!(
            format_args_culture(
                &smart,
                "{,10:L:pa{Tail}}",
                &map([("Tail", Value::from("per"))]),
                ""
            ),
            "padded literal and placeholder"
        );
    }

    /// The one divergence left in this formatter, pinned on our side here and
    /// on .NET's by the skipped golden
    /// `loc-nested-key-culture-sensitive-number`: .NET builds the isolated
    /// render's `FormatDetails` with a `null` provider, so a culture-sensitive
    /// placeholder in the key follows the ambient `CultureInfo.CurrentCulture`
    /// — probed, the same template looks up `1 234,50` with a French ambient
    /// culture and `1.234,50` with a German one, and the golden holds the
    /// invariant one the harness pins. There is no ambient culture here, so the
    /// render follows the culture in force. DESIGN.md, "Known divergences".
    #[test]
    fn a_culture_sensitive_nested_key_renders_with_the_culture_in_force() {
        let smart = smart_with(HashMapLocalizationProvider::from_triples([
            ("", "1.234,50", "the German key"),
            ("", "1,234.50", "the invariant key"),
        ]));
        let args = map([("Num", Value::from(1234.5f64))]);
        assert_eq!(
            format_args_culture(&smart, "{:L(de):{Num:N2}}", &args, ""),
            "the German key"
        );
        assert_eq!(
            format_args_culture(&smart, "{:L:{Num:N2}}", &args, ""),
            "the invariant key"
        );
    }

    #[test]
    fn the_isolated_render_keeps_the_collection_index() {
        // .NET holds the collection index in a `static`, which no
        // `FormatDetails` of its own can hide, so `{Index}` inside the isolated
        // render is the item's index (probed: looks up "0", then "1").
        let smart = smart_with(HashMapLocalizationProvider::from_triples([
            ("", "0", "first"),
            ("", "1", "second"),
        ]));
        let args = Value::List(vec![Value::List(vec![Value::from("a"), Value::from("b")])]);
        assert_eq!(
            format_args_culture(&smart, "{0:list:{:L:{Index}}|,}", &args, ""),
            "first,second"
        );
    }

    #[test]
    fn a_localized_string_may_localize_again() {
        // Probed: a `{:L:…}` inside a localized string resolves through the
        // same formatter, which the parse cache has to survive.
        let smart = smart_with(HashMapLocalizationProvider::from_triples([
            ("", "Outer", "<{:L:Inner}>"),
            ("", "Inner", "INNER"),
        ]));
        assert_eq!(format(&smart, "{:L:Outer}"), "<INNER>");
    }

    #[test]
    fn the_alignment_reaches_the_localized_string() {
        // Probed: `{,20:L(es):WeTranslateText}` pads to 20.
        let smart = smart();
        assert_eq!(
            format(&smart, "{,20:L(es):WeTranslateText}"),
            " Traducimos el texto"
        );
    }

    #[test]
    fn combine_with_conditional_formatter() {
        // .NET `Combine_With_ConditionalFormatter`. The conditional formatter
        // is in the default registry, so only the localization formatter is
        // added.
        let smart = smart();
        let template = "{0:cond:{:L:{} items}|{:L:{} item}|{:L:{} items}}";
        for (count, culture, expected) in [
            (0i64, "en", "0 items"),
            (1, "en", "1 item"),
            (200, "en", "200 items"),
            (200, "es", "200 elementos"),
            (200, "fr", "200 éléments"),
            (200, "de", "200 Elemente"),
        ] {
            let args = Value::List(vec![Value::from(count)]);
            assert_eq!(
                format_args_culture(&smart, template, &args, culture),
                expected,
                "{count} in {culture}"
            );
        }
    }

    #[cfg(feature = "plural")]
    #[test]
    fn combine_with_plural_localization_formatter() {
        // .NET `Combine_With_PluralLocalizationFormatter`.
        let smart = smart();
        let template = "{0:plural:{:L:{} item}|{:L:{} items}}";
        for (count, culture, expected) in [
            (0i64, "en", "0 items"),
            (1, "en", "1 item"),
            (200, "de", "200 Elemente"),
            (0, "fr", "0 élément"),
            (1, "fr", "1 élément"),
            (200, "fr", "200 éléments"),
        ] {
            let args = Value::List(vec![Value::from(count)]);
            assert_eq!(
                format_args_culture(&smart, template, &args, culture),
                expected,
                "{count} in {culture}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The parse cache
    // -----------------------------------------------------------------------

    #[test]
    fn a_localized_string_is_parsed_once() {
        // .NET `Should_Use_Existing_Localized_Format`. The registry hands out
        // `&dyn Formatter` and there is no downcast, so the cache is inspected
        // on a formatter of its own — the same code path the registered one
        // runs.
        let smart = SmartFormatter::default();
        let formatter = LocalizationFormatter::new(smart.parser(), Box::new(provider()));
        let first = formatter.parsed("Traducimos el texto").expect("parses");
        let again = formatter.parsed("Traducimos el texto").expect("parses");
        let other = formatter.parsed("Wir übersetzen Text").expect("parses");

        assert!(Arc::ptr_eq(&first, &again), "the second lookup reparsed");
        assert!(!Arc::ptr_eq(&first, &other));
        assert_eq!(formatter.cached_len(), 2);
    }

    #[test]
    fn the_cache_collides_when_names_are_matched_case_insensitively() {
        // Probed against 3.6.1: the cache is keyed with the host's comparer, so
        // two translations that differ only in case are one entry and the first
        // one parsed wins. A .NET bug, reproduced.
        let translations = [("", "K1", "abc {0}"), ("", "K2", "ABC {0}")];
        let args = Value::List(vec![Value::from("x")]);

        let insensitive = smart_with_settings(
            HashMapLocalizationProvider::from_triples(translations),
            SmartSettings {
                case_sensitive: CaseSensitivity::CaseInsensitive,
                ..SmartSettings::default()
            },
        );
        assert_eq!(
            format_args_culture(&insensitive, "{:L:K1}", &args, ""),
            "abc x"
        );
        assert_eq!(
            format_args_culture(&insensitive, "{:L:K2}", &args, ""),
            "abc x"
        );

        let sensitive = smart_with(HashMapLocalizationProvider::from_triples(translations));
        assert_eq!(
            format_args_culture(&sensitive, "{:L:K1}", &args, ""),
            "abc x"
        );
        assert_eq!(
            format_args_culture(&sensitive, "{:L:K2}", &args, ""),
            "ABC x"
        );
    }

    #[test]
    fn setting_a_provider_empties_the_cache() {
        let smart = SmartFormatter::default();
        let mut formatter = LocalizationFormatter::new(smart.parser(), Box::new(provider()));
        assert!(formatter.parsed("some string").is_ok());
        assert_eq!(formatter.cached_len(), 1);
        formatter.set_provider(Box::new(HashMapLocalizationProvider::new()));
        assert_eq!(formatter.cached_len(), 0);
    }

    // -----------------------------------------------------------------------
    // The provider
    // -----------------------------------------------------------------------

    #[test]
    fn the_provider_walks_the_culture_chain() {
        // .NET's `ResourceManager` chain: specific culture, its parents, then
        // the invariant culture.
        let provider = HashMapLocalizationProvider::from_triples([
            ("", "k", "invariant"),
            ("es", "k", "spanish"),
            ("es-MX", "k", "mexican"),
        ]);
        for (culture, expected) in [
            ("es-MX", "mexican"),
            ("es-ES", "spanish"),
            ("es", "spanish"),
            ("de", "invariant"),
            ("", "invariant"),
        ] {
            let data = culture::get(culture).expect("a shipped culture");
            assert_eq!(
                provider.get_string("k", data).as_deref(),
                Some(expected),
                "{culture}"
            );
        }
        assert_eq!(provider.get_string("nope", culture::invariant()), None);
    }

    #[test]
    fn the_fallback_culture_is_a_chain_of_its_own() {
        // .NET `GetString_UsesFallbackCulture_WhenValueNotFoundInRequestedCulture`.
        let provider = HashMapLocalizationProvider::from_triples([("en-US", "Greeting", "Hello")])
            .with_fallback_culture(culture::get("en-US").expect("a shipped culture"));
        let french = culture::get("fr-FR").expect("a shipped culture");
        assert_eq!(
            provider.get_string("Greeting", french).as_deref(),
            Some("Hello")
        );
        assert_eq!(provider.get_string("Missing", french), None);
    }

    #[test]
    fn return_name_if_not_found_answers_with_the_key() {
        // .NET `GetString_ReturnsKey_WhenResourceNotFound_AndReturnNameIfNotFoundIsTrue`.
        let provider = HashMapLocalizationProvider::new().with_return_name_if_not_found(true);
        assert_eq!(
            provider
                .get_string("NonExistingKey", culture::invariant())
                .as_deref(),
            Some("NonExistingKey")
        );
    }

    #[test]
    fn culture_names_are_canonicalized_on_insert() {
        let mut provider = HashMapLocalizationProvider::new();
        provider.insert("EN-us", "k", "one");
        assert!(provider.contains_culture("en-US"));
        assert_eq!(provider.get("en-us", "k"), Some("one"));
        // The second insert lands in the same table, as .NET's culture names
        // are matched case-insensitively.
        assert_eq!(provider.insert("en-US", "k", "two").as_deref(), Some("one"));
        assert_eq!(provider.len(), 1);

        // `_` names an alternate sort order, which .NET's data resolution
        // ignores: `es_ES` is the language `es`.
        provider.insert("es_ES", "k", "three");
        assert_eq!(provider.get("es", "k"), Some("three"));

        // A name no shipped culture matches is kept verbatim — and can then
        // never be reached through a `CultureData`.
        provider.insert("not-a-culture!", "k", "four");
        assert!(provider.contains_culture("not-a-culture!"));
        assert!(!provider.is_empty());
        provider.clear();
        assert!(provider.is_empty());
    }

    #[test]
    fn resource_names_are_matched_case_sensitively() {
        let provider = HashMapLocalizationProvider::from_triples([("", "Key", "value")]);
        assert_eq!(
            provider.get_string("Key", culture::invariant()).as_deref(),
            Some("value")
        );
        assert_eq!(provider.get_string("key", culture::invariant()), None);
    }

    /// A divergence, and the same one `TemplateFormatter` has: .NET builds the
    /// `FormattingException` from the failing item's own `BaseString`, which
    /// for a placeholder parsed out of a translation is *the translation*, so
    /// under `OutputErrorInResult` .NET quotes `Hello {Nope}!` where the engine
    /// here quotes the template it was called with. The issue and the index
    /// agree — the index is an offset into the translation on both sides,
    /// which is why our caret lands on the wrong character of the line we
    /// quote. DESIGN.md, "Known divergences".
    #[test]
    fn an_error_inside_a_translation_is_reported_against_the_outer_template() {
        let smart = smart_with_settings(
            HashMapLocalizationProvider::from_triples([("", "greetNobody", "Hello {Nope}!")]),
            SmartSettings {
                format_error_action: ErrorAction::OutputErrorInResult,
                ..SmartSettings::default()
            },
        );

        let rendered = format(&smart, "{:L:greetNobody}");
        assert_eq!(
            rendered,
            concat!(
                "Hello Error parsing format string: No source extension could handle ",
                "the selector named \"Nope\" at 7\n{:L:greetNobody}\n-------^!"
            )
        );
    }

    /// The chain is .NET's `CultureInfo.Parent`, which is *not* "drop the last
    /// subtag" for a language written in more than one script: probed on
    /// .NET 10, `zh-CN`'s parent is `zh-Hans`, and an `ILocalizationProvider`
    /// backed by `ResourceManager` finds a `zh-Hans` translation from `zh-CN`.
    /// Both cultures are in the generated table, so this is reachable with the
    /// data this crate ships.
    #[test]
    fn the_culture_chain_walks_dotnets_parents() {
        let provider = HashMapLocalizationProvider::from_triples([
            ("zh-Hans", "Hello", "\u{4f60}\u{597d}"),
            ("", "Hello", "Hello"),
        ]);
        assert_eq!(
            provider
                .get_string("Hello", culture::get("zh-CN").expect("zh-CN"))
                .as_deref(),
            Some("\u{4f60}\u{597d}")
        );
        // A culture with no script culture between it and its language still
        // walks one subtag at a time.
        assert_eq!(
            provider
                .get_string("Hello", culture::get("es-MX").expect("es-MX"))
                .as_deref(),
            Some("Hello")
        );
    }

    #[test]
    fn a_custom_provider_is_all_the_trait_needs() {
        // .NET `LocalizationProviderTests.CustomLocalizationProvider`, whose
        // provider keys on the two-letter language name.
        struct ByLanguage;
        impl LocalizationProvider for ByLanguage {
            fn get_string(&self, name: &str, culture: &CultureData) -> Option<Cow<'_, str>> {
                let language = culture.name.split('-').next().unwrap_or_default();
                match (language, name) {
                    ("en", "COUNTRY") => Some(Cow::Borrowed("country")),
                    ("fr", "COUNTRY") => Some(Cow::Borrowed("pays")),
                    ("es", "COUNTRY") => Some(Cow::Borrowed("país")),
                    _ => None,
                }
            }
        }

        let smart = smart_with(ByLanguage);
        assert_eq!(
            format(
                &smart,
                "{:L(en):COUNTRY} * {:L(fr):COUNTRY} * {:L(es):COUNTRY}"
            ),
            "country * pays * país"
        );
    }

    #[test]
    fn an_empty_registry_still_finds_the_formatter() {
        let mut smart = SmartFormatter::default();
        let formatter = LocalizationFormatter::new(smart.parser(), Box::new(provider()));
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(crate::formatter::DefaultFormatter));
        assert_eq!(
            format(&smart, "{:L(fr):WeTranslateText}"),
            "Nous traduisons des textes"
        );
    }
}
