//! The flags that drive time formatting, ported from
//! `SmartFormat.Extensions.Time/Utilities/TimeSpanFormatOptions.cs` and
//! `Utilities/TimeSpanFormatOptionsConverter.cs`.

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign};

/// The options a time format carries, a port of .NET's `[Flags]`
/// `TimeSpanFormatOptions`.
///
/// One value holds four independent settings, each a group of mutually
/// exclusive flags:
///
/// * abbreviation — [`ABBREVIATE`](Self::ABBREVIATE) / [`ABBREVIATE_OFF`](Self::ABBREVIATE_OFF)
/// * the "less than" text — [`LESS_THAN`](Self::LESS_THAN) / [`LESS_THAN_OFF`](Self::LESS_THAN_OFF)
/// * truncation — [`TRUNCATE_SHORTEST`](Self::TRUNCATE_SHORTEST) /
///   [`TRUNCATE_AUTO`](Self::TRUNCATE_AUTO) / [`TRUNCATE_FILL`](Self::TRUNCATE_FILL) /
///   [`TRUNCATE_FULL`](Self::TRUNCATE_FULL)
/// * the range of units, whose lowest and highest set flag are the smallest
///   and largest unit that may be written
///
/// A group that is empty is filled in from the defaults; see
/// [`merge`](Self::merge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TimeSpanFormatOptions(u32);

impl TimeSpanFormatOptions {
    /// Every setting is inherited from the defaults (.NET `None`).
    pub const NONE: Self = Self(0x0);

    /// Abbreviates units: `1d 2h 3m 4s 5ms`.
    pub const ABBREVIATE: Self = Self(0x1);
    /// Spells units out: `1 day 2 hours 3 minutes 4 seconds 5 milliseconds`.
    pub const ABBREVIATE_OFF: Self = Self(0x2);

    /// Writes "less than 1 (unit)" for a value below the smallest unit.
    pub const LESS_THAN: Self = Self(0x4);
    /// Writes "0 (units)" for a value below the smallest unit.
    pub const LESS_THAN_OFF: Self = Self(0x8);

    /// Writes the largest non-zero value in the range and nothing else:
    /// `00.23:00:59.000` is `23 hours`.
    pub const TRUNCATE_SHORTEST: Self = Self(0x10);
    /// Writes every non-zero value in the range: `23 hours 59 minutes`.
    pub const TRUNCATE_AUTO: Self = Self(0x20);
    /// Writes the largest non-zero value and every lesser one:
    /// `23 hours 0 minutes 59 seconds 0 milliseconds`.
    pub const TRUNCATE_FILL: Self = Self(0x40);
    /// Writes every value in the range:
    /// `0 days 23 hours 0 minutes 59 seconds 0 milliseconds`.
    pub const TRUNCATE_FULL: Self = Self(0x80);

    /// Milliseconds may be written. The smallest range flag.
    pub const RANGE_MILLI_SECONDS: Self = Self(0x100);
    /// Seconds may be written.
    pub const RANGE_SECONDS: Self = Self(0x200);
    /// Minutes may be written.
    pub const RANGE_MINUTES: Self = Self(0x400);
    /// Hours may be written.
    pub const RANGE_HOURS: Self = Self(0x800);
    /// Days may be written.
    pub const RANGE_DAYS: Self = Self(0x1000);
    /// Weeks may be written. The largest range flag.
    pub const RANGE_WEEKS: Self = Self(0x2000);

    /// The abbreviation group (.NET `TimeSpanFormatOptionsPresets.Abbreviate`).
    pub const ABBREVIATE_MASK: Self = Self(Self::ABBREVIATE.0 | Self::ABBREVIATE_OFF.0);
    /// The "less than" group (.NET `TimeSpanFormatOptionsPresets.LessThan`).
    pub const LESS_THAN_MASK: Self = Self(Self::LESS_THAN.0 | Self::LESS_THAN_OFF.0);
    /// The truncation group (.NET `TimeSpanFormatOptionsPresets.Truncate`).
    pub const TRUNCATE_MASK: Self = Self(
        Self::TRUNCATE_SHORTEST.0
            | Self::TRUNCATE_AUTO.0
            | Self::TRUNCATE_FILL.0
            | Self::TRUNCATE_FULL.0,
    );
    /// The range group (.NET `TimeSpanFormatOptionsPresets.Range`).
    pub const RANGE_MASK: Self = Self(
        Self::RANGE_MILLI_SECONDS.0
            | Self::RANGE_SECONDS.0
            | Self::RANGE_MINUTES.0
            | Self::RANGE_HOURS.0
            | Self::RANGE_DAYS.0
            | Self::RANGE_WEEKS.0,
    );

    /// The four groups, in the order .NET's `Merge` walks them.
    const GROUPS: [Self; 4] = [
        Self::ABBREVIATE_MASK,
        Self::LESS_THAN_MASK,
        Self::RANGE_MASK,
        Self::TRUNCATE_MASK,
    ];

    /// The raw flag bits, which are .NET's enumeration values.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// A value from raw flag bits. Bits that name no flag are kept, as they
    /// are in .NET, where the enum is just an `int`.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Whether every flag of `other` is set here.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no flag at all is set (.NET `== TimeSpanFormatOptions.None`).
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The flags of `mask` that are set here (.NET `Mask`).
    pub const fn mask(self, mask: Self) -> Self {
        Self(self.0 & mask.0)
    }

    /// Fills in every group this value leaves empty from `right`
    /// (.NET `Merge`).
    ///
    /// A group is all-or-nothing: `{0:time:hours}` names a range and inherits
    /// the truncation, abbreviation and "less than" settings, and naming *any*
    /// range flag replaces the default range entirely.
    pub fn merge(self, right: Self) -> Self {
        let mut merged = self;
        for group in Self::GROUPS {
            if merged.mask(group).is_empty() {
                merged |= right.mask(group);
            }
        }
        merged
    }

    /// The flags that are set, lowest bit first (.NET `AllFlags`).
    pub fn all_flags(self) -> impl Iterator<Item = Self> {
        let bits = self.0;
        (0..u32::BITS)
            .map(|shift| Self(1 << shift))
            .filter(move |flag| flag.0 & bits != 0)
    }

    /// The lowest flag that is set (.NET `AllFlags().First()`, which throws
    /// where this is `None`).
    pub fn first_flag(self) -> Option<Self> {
        self.all_flags().next()
    }

    /// The highest flag that is set (.NET `AllFlags().Last()`).
    pub fn last_flag(self) -> Option<Self> {
        self.all_flags().last()
    }

    /// The next flag down, which for a range flag is the next smaller unit
    /// (.NET's `i = (TimeSpanFormatOptions)((int)i >> 1)`).
    pub(crate) const fn shift_down(self) -> Self {
        Self(self.0 >> 1)
    }
}

impl BitOr for TimeSpanFormatOptions {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for TimeSpanFormatOptions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for TimeSpanFormatOptions {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl fmt::Display for TimeSpanFormatOptions {
    /// The flag names, `|`-separated, as .NET's `[Flags]` `ToString()` writes
    /// them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return f.write_str("None");
        }
        let mut first = true;
        for flag in self.all_flags() {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            f.write_str(flag_name(flag))?;
        }
        Ok(())
    }
}

/// The .NET name of a single flag, or its bit value for one that has no name.
fn flag_name(flag: TimeSpanFormatOptions) -> &'static str {
    match flag {
        TimeSpanFormatOptions::ABBREVIATE => "Abbreviate",
        TimeSpanFormatOptions::ABBREVIATE_OFF => "AbbreviateOff",
        TimeSpanFormatOptions::LESS_THAN => "LessThan",
        TimeSpanFormatOptions::LESS_THAN_OFF => "LessThanOff",
        TimeSpanFormatOptions::TRUNCATE_SHORTEST => "TruncateShortest",
        TimeSpanFormatOptions::TRUNCATE_AUTO => "TruncateAuto",
        TimeSpanFormatOptions::TRUNCATE_FILL => "TruncateFill",
        TimeSpanFormatOptions::TRUNCATE_FULL => "TruncateFull",
        TimeSpanFormatOptions::RANGE_MILLI_SECONDS => "RangeMilliSeconds",
        TimeSpanFormatOptions::RANGE_SECONDS => "RangeSeconds",
        TimeSpanFormatOptions::RANGE_MINUTES => "RangeMinutes",
        TimeSpanFormatOptions::RANGE_HOURS => "RangeHours",
        TimeSpanFormatOptions::RANGE_DAYS => "RangeDays",
        TimeSpanFormatOptions::RANGE_WEEKS => "RangeWeeks",
        _ => "?",
    }
}

/// Reads the options out of a time format, a port of
/// `TimeSpanFormatOptionsConverter.Parse`.
///
/// .NET matches the keywords with the regular expression
/// `\b(w|week|weeks|d|day|days|h|hour|hours|m|minute|minutes|s|second|seconds
/// |ms|millisecond|milliseconds|auto|short|fill|full|abbr|noabbr|less|noless)\b`
/// over the lowercased text. Every alternative is made of word characters, and
/// the word boundaries on both sides mean a match has to cover a whole run of
/// them — so this walks the runs instead of running a regular expression, and
/// keeps the `time` extension free of the regex dependency.
///
/// Anything that is not a keyword is ignored, exactly as there:
/// `{0:time:the hours and minutes}` is a range of hours to minutes.
///
/// Two hairs' worth of divergence, both in what counts as a word character:
///
/// * .NET lowercases with the *thread* culture before matching, so a Turkish
///   thread turns `MILLISECONDS` into `mıllıseconds` and stops recognizing it.
///   We compare ASCII-case-insensitively, which is what every other culture
///   does.
/// * .NET's `\w` is `[\p{L}\p{Mn}\p{Nd}\p{Pc}]`; ours is
///   [`char::is_alphanumeric`] plus `_`. The two differ only for a combining
///   mark or an exotic numeral written up against a keyword — `hours\u{300}`
///   is one word to .NET and two to us.
pub fn parse(format_string: &str) -> TimeSpanFormatOptions {
    let mut options = TimeSpanFormatOptions::NONE;
    for word in words(format_string) {
        options |= keyword(word);
    }
    options
}

/// The runs of word characters in `text`, which are the only places a keyword
/// can match.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !is_word_char(c))
        .filter(|word| !word.is_empty())
}

/// A character .NET's `\b` treats as part of a word.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The flag a whole word sets, or [`TimeSpanFormatOptions::NONE`] for a word
/// that is not a keyword.
fn keyword(word: &str) -> TimeSpanFormatOptions {
    const KEYWORDS: [(&str, TimeSpanFormatOptions); 26] = [
        ("w", TimeSpanFormatOptions::RANGE_WEEKS),
        ("week", TimeSpanFormatOptions::RANGE_WEEKS),
        ("weeks", TimeSpanFormatOptions::RANGE_WEEKS),
        ("d", TimeSpanFormatOptions::RANGE_DAYS),
        ("day", TimeSpanFormatOptions::RANGE_DAYS),
        ("days", TimeSpanFormatOptions::RANGE_DAYS),
        ("h", TimeSpanFormatOptions::RANGE_HOURS),
        ("hour", TimeSpanFormatOptions::RANGE_HOURS),
        ("hours", TimeSpanFormatOptions::RANGE_HOURS),
        ("m", TimeSpanFormatOptions::RANGE_MINUTES),
        ("minute", TimeSpanFormatOptions::RANGE_MINUTES),
        ("minutes", TimeSpanFormatOptions::RANGE_MINUTES),
        ("s", TimeSpanFormatOptions::RANGE_SECONDS),
        ("second", TimeSpanFormatOptions::RANGE_SECONDS),
        ("seconds", TimeSpanFormatOptions::RANGE_SECONDS),
        ("ms", TimeSpanFormatOptions::RANGE_MILLI_SECONDS),
        ("millisecond", TimeSpanFormatOptions::RANGE_MILLI_SECONDS),
        ("milliseconds", TimeSpanFormatOptions::RANGE_MILLI_SECONDS),
        ("short", TimeSpanFormatOptions::TRUNCATE_SHORTEST),
        ("auto", TimeSpanFormatOptions::TRUNCATE_AUTO),
        ("fill", TimeSpanFormatOptions::TRUNCATE_FILL),
        ("full", TimeSpanFormatOptions::TRUNCATE_FULL),
        ("abbr", TimeSpanFormatOptions::ABBREVIATE),
        ("noabbr", TimeSpanFormatOptions::ABBREVIATE_OFF),
        ("less", TimeSpanFormatOptions::LESS_THAN),
        ("noless", TimeSpanFormatOptions::LESS_THAN_OFF),
    ];

    KEYWORDS
        .iter()
        .find(|(keyword, _)| keyword.eq_ignore_ascii_case(word))
        .map_or(TimeSpanFormatOptions::NONE, |(_, flag)| *flag)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::TimeSpanFormatOptions as O;
    use super::*;

    #[test]
    fn flag_values_are_the_dotnet_enumeration() {
        assert_eq!(O::NONE.bits(), 0x0);
        assert_eq!(O::ABBREVIATE.bits(), 0x1);
        assert_eq!(O::ABBREVIATE_OFF.bits(), 0x2);
        assert_eq!(O::LESS_THAN.bits(), 0x4);
        assert_eq!(O::LESS_THAN_OFF.bits(), 0x8);
        assert_eq!(O::TRUNCATE_SHORTEST.bits(), 0x10);
        assert_eq!(O::TRUNCATE_AUTO.bits(), 0x20);
        assert_eq!(O::TRUNCATE_FILL.bits(), 0x40);
        assert_eq!(O::TRUNCATE_FULL.bits(), 0x80);
        assert_eq!(O::RANGE_MILLI_SECONDS.bits(), 0x100);
        assert_eq!(O::RANGE_SECONDS.bits(), 0x200);
        assert_eq!(O::RANGE_MINUTES.bits(), 0x400);
        assert_eq!(O::RANGE_HOURS.bits(), 0x800);
        assert_eq!(O::RANGE_DAYS.bits(), 0x1000);
        assert_eq!(O::RANGE_WEEKS.bits(), 0x2000);
    }

    #[test]
    fn every_keyword_sets_its_flag() {
        for (text, expected) in [
            ("w", O::RANGE_WEEKS),
            ("week", O::RANGE_WEEKS),
            ("weeks", O::RANGE_WEEKS),
            ("d", O::RANGE_DAYS),
            ("day", O::RANGE_DAYS),
            ("days", O::RANGE_DAYS),
            ("h", O::RANGE_HOURS),
            ("hour", O::RANGE_HOURS),
            ("hours", O::RANGE_HOURS),
            ("m", O::RANGE_MINUTES),
            ("minute", O::RANGE_MINUTES),
            ("minutes", O::RANGE_MINUTES),
            ("s", O::RANGE_SECONDS),
            ("second", O::RANGE_SECONDS),
            ("seconds", O::RANGE_SECONDS),
            ("ms", O::RANGE_MILLI_SECONDS),
            ("millisecond", O::RANGE_MILLI_SECONDS),
            ("milliseconds", O::RANGE_MILLI_SECONDS),
            ("short", O::TRUNCATE_SHORTEST),
            ("auto", O::TRUNCATE_AUTO),
            ("fill", O::TRUNCATE_FILL),
            ("full", O::TRUNCATE_FULL),
            ("abbr", O::ABBREVIATE),
            ("noabbr", O::ABBREVIATE_OFF),
            ("less", O::LESS_THAN),
            ("noless", O::LESS_THAN_OFF),
        ] {
            assert_eq!(parse(text), expected, "{text}");
            assert_eq!(parse(&text.to_uppercase()), expected, "{text} uppercased");
        }
    }

    #[test]
    fn keywords_are_whole_words() {
        // Probed against 3.6.1: a keyword glued to anything else is not one.
        assert_eq!(parse("1hours"), O::NONE);
        assert_eq!(parse("hoursminutes"), O::NONE);
        assert_eq!(parse("shorter"), O::NONE);
        assert_eq!(parse("xyz"), O::NONE);
        assert_eq!(parse(""), O::NONE);
        assert_eq!(parse("   "), O::NONE);
        // Anything that is not a word character separates two words.
        assert_eq!(parse("hours,minutes"), O::RANGE_HOURS | O::RANGE_MINUTES);
        assert_eq!(parse("hours-minutes"), O::RANGE_HOURS | O::RANGE_MINUTES);
        assert_eq!(parse("hours minutes"), O::RANGE_HOURS | O::RANGE_MINUTES);
        assert_eq!(
            parse("the hours and minutes"),
            O::RANGE_HOURS | O::RANGE_MINUTES
        );
    }

    #[test]
    fn opposite_keywords_both_set_their_flag() {
        assert_eq!(parse("less noless"), O::LESS_THAN | O::LESS_THAN_OFF);
        assert_eq!(parse("abbr noabbr"), O::ABBREVIATE | O::ABBREVIATE_OFF);
        assert_eq!(parse("auto short"), O::TRUNCATE_AUTO | O::TRUNCATE_SHORTEST);
    }

    #[test]
    fn merge_fills_in_empty_groups_only() {
        let defaults =
            O::ABBREVIATE_OFF | O::LESS_THAN | O::TRUNCATE_AUTO | O::RANGE_SECONDS | O::RANGE_DAYS;

        // A named range replaces the whole default range.
        let hours = parse("hours").merge(defaults);
        assert_eq!(hours.mask(O::RANGE_MASK), O::RANGE_HOURS);
        assert_eq!(hours.mask(O::TRUNCATE_MASK), O::TRUNCATE_AUTO);
        assert_eq!(hours.mask(O::LESS_THAN_MASK), O::LESS_THAN);
        assert_eq!(hours.mask(O::ABBREVIATE_MASK), O::ABBREVIATE_OFF);

        // Nothing named at all inherits every group.
        assert_eq!(O::NONE.merge(defaults), defaults);
        // A group that is set is left alone.
        assert_eq!(
            parse("noless").merge(defaults).mask(O::LESS_THAN_MASK),
            O::LESS_THAN_OFF
        );
    }

    #[test]
    fn flags_come_out_lowest_first() {
        let range = parse("weeks milliseconds");
        assert_eq!(range.first_flag(), Some(O::RANGE_MILLI_SECONDS));
        assert_eq!(range.last_flag(), Some(O::RANGE_WEEKS));
        assert_eq!(O::NONE.first_flag(), None);
        assert_eq!(O::NONE.last_flag(), None);
        assert_eq!(
            parse("fill short").mask(O::TRUNCATE_MASK).first_flag(),
            Some(O::TRUNCATE_SHORTEST)
        );
    }

    #[test]
    fn display_names_the_flags() {
        assert_eq!(O::NONE.to_string(), "None");
        assert_eq!(O::RANGE_WEEKS.to_string(), "RangeWeeks");
        assert_eq!(
            (O::ABBREVIATE | O::RANGE_DAYS).to_string(),
            "Abbreviate, RangeDays"
        );
    }
}
