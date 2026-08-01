//! Culture data needed by the standard format specifiers, mirroring the
//! parts of .NET `NumberFormatInfo` / `DateTimeFormatInfo` we consume.
//!
//! Every culture but the invariant one lives in the private `generated`
//! module (`generated.rs`), read straight
//! out of a real .NET `CultureInfo` by `tools/culturegen` rather than mapped
//! from CLDR, so a listed culture formats byte-identically to .NET by
//! construction. Pattern integers (`currency_negative_pattern` etc.) are the
//! exact .NET enumeration values, so ported formatting code can follow the
//! .NET reference directly.

mod generated;

/// Number formatting symbols and patterns (.NET `NumberFormatInfo` subset).
#[derive(Debug, Clone, PartialEq)]
pub struct NumberFormat {
    pub decimal_separator: &'static str,
    pub group_separator: &'static str,
    /// Digits per group, least-significant first (.NET `NumberGroupSizes`).
    pub group_sizes: &'static [u8],
    pub negative_sign: &'static str,
    /// .NET `PositiveSign`, used for a non-negative exponent in `E` notation.
    /// Not always `"+"`: every `ar-*` culture prefixes it with a bidi mark.
    pub positive_sign: &'static str,
    pub number_decimal_digits: u8,
    /// .NET `NumberNegativePattern` (0..=4), which `N` applies to a negative
    /// value. Every culture we ship uses `1` (`-n`), but the invariant-only
    /// assumption is exactly the kind that bites later, so it is real data.
    pub number_negative_pattern: u8,
    pub currency_symbol: &'static str,
    pub currency_decimal_digits: u8,
    pub currency_decimal_separator: &'static str,
    pub currency_group_separator: &'static str,
    /// .NET `CurrencyGroupSizes`, which is not always `NumberGroupSizes`.
    pub currency_group_sizes: &'static [u8],
    /// .NET `CurrencyPositivePattern` (0 = `$n`, 1 = `n$`, 2 = `$ n`, 3 = `n $`).
    pub currency_positive_pattern: u8,
    /// .NET `CurrencyNegativePattern` (0..=16).
    pub currency_negative_pattern: u8,
    pub percent_symbol: &'static str,
    pub percent_decimal_digits: u8,
    pub percent_decimal_separator: &'static str,
    pub percent_group_separator: &'static str,
    /// .NET `PercentGroupSizes`.
    pub percent_group_sizes: &'static [u8],
    /// .NET `PercentPositivePattern` / `PercentNegativePattern`.
    pub percent_positive_pattern: u8,
    pub percent_negative_pattern: u8,
    pub nan_symbol: &'static str,
    pub positive_infinity_symbol: &'static str,
    pub negative_infinity_symbol: &'static str,
}

/// Date/time symbols and standard patterns (.NET `DateTimeFormatInfo` subset).
#[derive(Debug, Clone, PartialEq)]
pub struct DateTimeFormat {
    /// January first, 12 entries (.NET has a 13th empty slot; we don't).
    pub month_names: [&'static str; 12],
    pub abbreviated_month_names: [&'static str; 12],
    /// .NET `MonthGenitiveNames`: the form a month takes next to a day number,
    /// which Slavic and Finnic cultures inflect (`ru` "март" but "5 марта").
    pub month_genitive_names: [&'static str; 12],
    pub abbreviated_month_genitive_names: [&'static str; 12],
    /// .NET `DateTimeFormatFlags.UseGenitiveMonth`, which
    /// `DateTimeFormatInfoScanner.GetFormatFlagGenitiveMonth` sets exactly when
    /// a culture's genitive names differ from its regular ones. Cultures
    /// without the flag never consult the genitive arrays, so `de`'s
    /// abbreviated genitive "März" only shows up because `de` *does* have it.
    pub use_genitive_month: bool,
    /// Sunday first, matching .NET `DayNames`.
    pub day_names: [&'static str; 7],
    pub abbreviated_day_names: [&'static str; 7],
    /// .NET `Calendar.GetEraName`, which the `g` pattern token renders. The
    /// invariant Gregorian calendar has the single era `"A.D."`.
    pub era_name: &'static str,
    pub am_designator: &'static str,
    pub pm_designator: &'static str,
    pub date_separator: &'static str,
    pub time_separator: &'static str,
    /// Custom-pattern strings backing the standard specifiers
    /// (`d` → `short_date_pattern`, `D` → `long_date_pattern`, …).
    pub short_date_pattern: &'static str,
    pub long_date_pattern: &'static str,
    pub short_time_pattern: &'static str,
    pub long_time_pattern: &'static str,
    pub month_day_pattern: &'static str,
    pub year_month_pattern: &'static str,
    pub full_date_time_pattern: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CultureData {
    /// BCP-47-ish name; `""` for the invariant culture, like .NET.
    pub name: &'static str,
    pub number: NumberFormat,
    pub datetime: DateTimeFormat,
}

/// The .NET invariant culture (`CultureInfo.InvariantCulture`).
pub fn invariant() -> &'static CultureData {
    &INVARIANT
}

/// Looks up a culture by name (`""` → invariant, `"de-DE"`, `"is"`, …),
/// matching the name case-insensitively like .NET's `GetCultureInfo`, and
/// ignoring an alternate sort order the way .NET's *data* resolution does:
/// everything from the `_` on names a collation, not a culture, so `"en_US"`
/// is the language `en` sorted the American way and formats as `en` — with the
/// invariant currency symbol `¤`, not `en-US`'s `$`. Probed against .NET 10:
/// `GetCultureInfo("en_US")` and `GetCultureInfo("en")` agree in every
/// `NumberFormat` and `DateTimeFormat` field this crate carries, as do
/// `"de-DE_phoneb"` and `"de-DE"`.
///
/// A name .NET itself rejects (see `language_subtag` in this module) is `None`
/// rather than a lookup of some prefix of it.
///
/// The data is generated from .NET itself by `tools/culturegen` (not mapped
/// from CLDR), so a listed culture formats byte-identically to .NET; `None`
/// means the culture is not in the generated set — the caller decides whether
/// to error or fall back, we never guess at data. .NET would answer a name
/// outside the set from the CLDR tree (`"zh_Hans"` is `zh` there, a culture we
/// do not ship); we say we do not know it.
pub fn get(name: &str) -> Option<&'static CultureData> {
    if name.is_empty() {
        return Some(&INVARIANT);
    }
    language_subtag(name)?;
    let culture = name
        .split_once(SORT_ORDER_SEPARATOR)
        .map_or(name, |(culture, _sort_order)| culture);
    generated::lookup(culture)
}

/// The name of the culture .NET's `CultureInfo.Parent` gives this one, `""`
/// for a neutral culture (whose parent is the invariant culture) and for the
/// invariant culture itself.
///
/// It is *usually* the name with its last subtag dropped, but not always: a
/// language written in more than one script has a neutral culture per script
/// between the specific culture and the language, so `zh-CN` walks up through
/// `zh-Hans` and not straight to `zh` (`SCRIPT_PARENTS` in this module).
/// Anything that walks the chain — [`LocalizationProvider`], which stands in
/// for .NET's `ResourceManager.GetString(name, culture)` — has to walk .NET's,
/// or a translation filed under the script culture is unreachable from the
/// specific one.
///
/// [`LocalizationProvider`]: crate::extensions::localization::LocalizationProvider
pub fn parent_name(name: &str) -> &str {
    if let Some((_, parent)) = SCRIPT_PARENTS
        .iter()
        .find(|(child, _)| child.eq_ignore_ascii_case(name))
    {
        return parent;
    }
    match name.rfind('-') {
        Some(separator) => &name[..separator],
        None => "",
    }
}

/// The cultures whose `CultureInfo.Parent` is *not* the name with its last
/// subtag dropped, probed on .NET 10 (ICU). The list covers the one such
/// culture this crate ships (`zh-CN`) and its siblings, which cost nothing to
/// carry and are what a caller reaching for a Chinese name is likely to write;
/// `parents_are_dotnets` pins the parent of every generated culture, so adding
/// one whose parent needs an entry here fails that test rather than silently
/// walking the wrong chain.
///
/// The legacy aliases `zh-CHS` and `zh-CHT` need no entry: .NET answers `zh`
/// for both, which is what dropping the last subtag gives.
const SCRIPT_PARENTS: &[(&str, &str)] = &[
    ("zh-CN", "zh-Hans"),
    ("zh-SG", "zh-Hans"),
    ("zh-TW", "zh-Hant"),
    ("zh-HK", "zh-Hant"),
    ("zh-MO", "zh-Hant"),
];

/// The characters .NET allows between the subtags of a culture name: `-`
/// between the subtags proper, and one [`SORT_ORDER_SEPARATOR`] before the
/// name of an alternate sort order (`de-DE_phoneb`).
pub(crate) const SUBTAG_SEPARATORS: [char; 2] = ['-', SORT_ORDER_SEPARATOR];

/// What separates a culture name from the alternate sort order after it.
pub(crate) const SORT_ORDER_SEPARATOR: char = '_';

/// `CultureInfo.GetCultureInfo`'s longest accepted name, .NET
/// `CultureData.LocaleNameMaxLength`.
const LOCALE_NAME_MAX_LENGTH: usize = 85;

/// ICU's longest accepted language subtag, `ULOC_LANG_CAPACITY` minus the
/// terminator (probed: `aaaaaaaaaaa` resolves, `aaaaaaaaaaaa` does not).
const LANGUAGE_SUBTAG_MAX_LENGTH: usize = 11;

/// The primary subtag of a culture name .NET's `CultureInfo.GetCultureInfo`
/// accepts — the language, as it was spelled — or `None` for a name .NET
/// rejects with `CultureNotFoundException`.
///
/// This is the one place the crate decides what a .NET culture name is:
/// [`get`] uses it before looking a name up, and the `plural(…)` option uses
/// it to name a language.
///
/// .NET checks the characters itself (`CultureData.IcuIsValidCultureName`:
/// ASCII letters and digits, `-`, and at most one `_`) and leaves the rest to
/// ICU. Probed against .NET 10, ICU additionally rejects an empty subtag
/// (`en-`, `en--US`, `_en`), a language subtag longer than
/// [`LANGUAGE_SUBTAG_MAX_LENGTH`], and a name that is one character long (`a`,
/// though `a-b` is accepted).
///
/// Lowercased, the subtag is the `TwoLetterISOLanguageName` of every culture
/// .NET knows (probed over `CultureInfo.GetCultures(AllCultures)`), which is
/// what the `plural(…)` option needs. Two ICU resolutions it does not
/// reproduce, both in DESIGN.md: a three-letter ISO 639-2 code that has a
/// two-letter equivalent (`eng` is `en` in .NET, itself here), and `und` (the
/// invariant culture, whose language is `iv`).
pub(crate) fn language_subtag(name: &str) -> Option<&str> {
    if name.is_empty() || name.len() > LOCALE_NAME_MAX_LENGTH {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || SUBTAG_SEPARATORS.contains(&c))
    {
        return None;
    }
    if name.matches(SORT_ORDER_SEPARATOR).count() > 1 {
        return None;
    }

    let mut subtags = name.split(SUBTAG_SEPARATORS);
    let language = subtags.next().unwrap_or_default();
    if language.len() > LANGUAGE_SUBTAG_MAX_LENGTH {
        return None;
    }
    let mut has_more = false;
    for subtag in subtags {
        has_more = true;
        if subtag.is_empty() {
            return None;
        }
    }
    // A name of one character is a culture only with a subtag after it.
    (language.len() >= if has_more { 1 } else { 2 }).then_some(language)
}

static INVARIANT: CultureData = CultureData {
    name: "",
    number: NumberFormat {
        decimal_separator: ".",
        group_separator: ",",
        group_sizes: &[3],
        negative_sign: "-",
        positive_sign: "+",
        number_decimal_digits: 2,
        number_negative_pattern: 1,
        currency_symbol: "\u{a4}",
        currency_decimal_digits: 2,
        currency_decimal_separator: ".",
        currency_group_separator: ",",
        currency_group_sizes: &[3],
        currency_positive_pattern: 0,
        currency_negative_pattern: 0,
        percent_symbol: "%",
        percent_decimal_digits: 2,
        percent_decimal_separator: ".",
        percent_group_separator: ",",
        percent_group_sizes: &[3],
        percent_positive_pattern: 0,
        percent_negative_pattern: 0,
        nan_symbol: "NaN",
        positive_infinity_symbol: "Infinity",
        negative_infinity_symbol: "-Infinity",
    },
    datetime: DateTimeFormat {
        month_names: [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
        abbreviated_month_names: [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ],
        month_genitive_names: [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
        abbreviated_month_genitive_names: [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ],
        use_genitive_month: false,
        day_names: [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ],
        abbreviated_day_names: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
        era_name: "A.D.",
        am_designator: "AM",
        pm_designator: "PM",
        date_separator: "/",
        time_separator: ":",
        short_date_pattern: "MM/dd/yyyy",
        long_date_pattern: "dddd, dd MMMM yyyy",
        short_time_pattern: "HH:mm",
        long_time_pattern: "HH:mm:ss",
        month_day_pattern: "MMMM dd",
        year_month_pattern: "yyyy MMMM",
        full_date_time_pattern: "dddd, dd MMMM yyyy HH:mm:ss",
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    /// `""` is answered from the hand-written [`INVARIANT`], so the two copies
    /// have to agree — otherwise the invariant culture would format one way
    /// here and another way in the .NET the goldens came from.
    #[test]
    fn the_generated_invariant_matches_the_hand_written_one() {
        let generated = generated::CULTURES
            .iter()
            .find(|culture| culture.name.is_empty())
            .expect("culturegen emits the invariant culture");
        assert_eq!(*generated, INVARIANT);
    }

    /// [`generated::lookup`] binary-searches, so a mis-sorted table would make
    /// cultures randomly invisible rather than fail loudly.
    #[test]
    fn the_generated_table_is_sorted_by_lowercase_name() {
        let names: Vec<String> = generated::CULTURES
            .iter()
            .map(|culture| culture.name.to_ascii_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(names, sorted);
    }

    #[test]
    fn every_culture_in_the_table_is_reachable() {
        for culture in generated::CULTURES {
            let found = get(culture.name).expect(culture.name);
            assert_eq!(found.name, culture.name);
            assert_eq!(
                get(&culture.name.to_ascii_uppercase())
                    .expect(culture.name)
                    .name,
                culture.name
            );
        }
    }

    /// The parent of every culture this crate ships, probed on .NET 10 with
    /// `CultureInfo.GetCultureInfo(name).Parent.Name`. A culture added to
    /// `tools/culturegen` fails here until its parent is written down, which
    /// is the point: `zh-CN`'s parent is `zh-Hans`, not `zh`, and nothing in
    /// the name says so.
    #[test]
    fn parents_are_dotnets() {
        let parents: &[(&str, &str)] = &[
            ("", ""),
            ("ar", ""),
            ("ar-SA", "ar"),
            ("cs", ""),
            ("da", ""),
            ("de", ""),
            ("de-AT", "de"),
            ("de-CH", "de"),
            ("de-DE", "de"),
            ("en", ""),
            ("en-GB", "en"),
            ("en-US", "en"),
            ("es", ""),
            ("es-ES", "es"),
            ("es-MX", "es"),
            ("fi", ""),
            ("fr", ""),
            ("fr-FR", "fr"),
            ("is", ""),
            ("is-IS", "is"),
            ("it", ""),
            ("ja", ""),
            ("ko", ""),
            ("nb", ""),
            ("nl", ""),
            ("pl", ""),
            ("pt", ""),
            ("pt-BR", "pt"),
            ("pt-PT", "pt"),
            ("ru", ""),
            ("sv", ""),
            ("tr", ""),
            ("uk", ""),
            ("zh-CN", "zh-Hans"),
            ("zh-Hans", "zh"),
        ];

        let listed: Vec<&str> = generated::CULTURES.iter().map(|c| c.name).collect();
        let pinned: Vec<&str> = parents.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            listed, pinned,
            "the generated culture list changed; probe the new cultures' parents"
        );
        for (name, parent) in parents {
            assert_eq!(parent_name(name), *parent, "parent of {name:?}");
        }

        // The rest of the chain above a shipped culture, which is .NET's too
        // even where we ship no data for the culture itself.
        assert_eq!(parent_name("zh"), "");
        assert_eq!(parent_name("zh-TW"), "zh-Hant");
        assert_eq!(parent_name("zh-Hant"), "zh");
        assert_eq!(parent_name("sr-Latn-RS"), "sr-Latn");
        // Case-insensitively, as every other culture name lookup is.
        assert_eq!(parent_name("ZH-cn"), "zh-Hans");
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert_eq!(get("de-DE").expect("de-DE").name, "de-DE");
        assert_eq!(get("DE-de").expect("DE-de").name, "de-DE");
        assert_eq!(get("dE-dE").expect("dE-dE").name, "de-DE");
        assert_eq!(get("ZH-hans").expect("ZH-hans").name, "zh-Hans");
    }

    #[test]
    fn the_empty_name_is_the_invariant_culture() {
        assert!(std::ptr::eq(get("").expect("invariant"), invariant()));
    }

    /// .NET would resolve an unknown name against the whole CLDR tree; we only
    /// have what `tools/culturegen` was asked for, so anything else is `None`
    /// rather than a guess at a parent's data. `zh_Hans` is the interesting
    /// one: .NET reads it as the neutral culture `zh`, which we do not ship,
    /// so it is unknown here even though `zh-Hans` is listed.
    #[test]
    fn an_unlisted_culture_is_none() {
        for name in [
            "de-XX",
            "d",
            "klingon",
            " de-DE",
            "de-DE ",
            "zh_Hans",
            "en-US-POSIX",
        ] {
            assert!(get(name).is_none(), "{name}");
        }
    }

    /// A name .NET itself rejects is unknown, not a lookup of the part of it
    /// before the sort order — `en_` must not find `en`.
    #[test]
    fn a_name_dotnet_rejects_is_none() {
        for name in [
            "de-",
            "en_",
            "_en",
            "en--US",
            "en-US_",
            "en_US_POSIX",
            "a",
            "@@",
            "de DE",
            "рус",
            "aaaaaaaaaaaa",
        ] {
            assert!(language_subtag(name).is_none(), "{name}");
            assert!(get(name).is_none(), "{name}");
        }
    }

    /// .NET reads the text after an `_` as an alternate sort order, which
    /// changes how strings compare and nothing about how values format: probed
    /// on .NET 10, `en_US` has every `NumberFormat` and `DateTimeFormat` field
    /// of `en` (currency `¤`), not of `en-US` (currency `$`).
    #[test]
    fn an_alternate_sort_order_resolves_to_the_culture_before_it() {
        for (name, culture) in [
            ("en_US", "en"),
            ("EN_us", "en"),
            ("en_US-POSIX", "en"),
            ("is_IS", "is"),
            ("de-DE_phoneb", "de-DE"),
            ("pt-BR_zzz", "pt-BR"),
        ] {
            let found = get(name).unwrap_or_else(|| panic!("{name}"));
            assert!(std::ptr::eq(found, get(culture).expect(culture)), "{name}");
            assert_eq!(found.name, culture, "{name}");
        }
    }

    /// The names of the table are names .NET spelled, so the validator that
    /// guards [`get`] must not hide any of them.
    #[test]
    fn every_culture_in_the_table_is_a_valid_name() {
        for culture in generated::CULTURES {
            if culture.name.is_empty() {
                continue;
            }
            let language = language_subtag(culture.name).expect(culture.name);
            assert!(culture.name.starts_with(language), "{}", culture.name);
        }
    }

    /// The language subtag is the name as written up to the first separator;
    /// lowercased it is .NET's `TwoLetterISOLanguageName` (probed).
    #[test]
    fn the_language_subtag_is_the_primary_one() {
        for (name, language) in [
            ("en", "en"),
            ("EN", "EN"),
            ("en-US", "en"),
            ("en_US", "en"),
            ("zh-Hans", "zh"),
            ("a-b", "a"),
            ("1234", "1234"),
            ("kde", "kde"),
            ("aaaaaaaaaaa", "aaaaaaaaaaa"),
            ("en-us-x-private", "en"),
        ] {
            assert_eq!(language_subtag(name), Some(language), "{name}");
        }
    }
}
