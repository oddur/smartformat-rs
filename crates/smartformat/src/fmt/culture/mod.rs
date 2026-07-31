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
/// matching the name case-insensitively like .NET's `GetCultureInfo`.
///
/// The data is generated from .NET itself by `tools/culturegen` (not mapped
/// from CLDR), so a listed culture formats byte-identically to .NET; `None`
/// means the culture is not in the generated set — the caller decides whether
/// to error or fall back, we never guess at data.
pub fn get(name: &str) -> Option<&'static CultureData> {
    if name.is_empty() {
        return Some(&INVARIANT);
    }
    generated::lookup(name)
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
    /// rather than a guess at a parent's data.
    #[test]
    fn an_unlisted_culture_is_none() {
        for name in ["de-XX", "de-", "d", "en_US", "klingon", " de-DE", "de-DE "] {
            assert!(get(name).is_none(), "{name}");
        }
    }
}
