//! Port of SmartFormat.NET's `ListFormatter` — both formatter and source (milestone M3).
//!
//! Ported from `src/SmartFormat/Extensions/ListFormatter.cs`. The source half —
//! the `{Index}` selector, and indexing a second list with it — is
//! [`ListSource`](crate::sources::ListSource): a Rust extension is registered by
//! value in one registry or the other, and the two halves share nothing but the
//! collection index, which the engine holds
//! ([`FormattingInfo::collection_index`]).
//!
//! `{0:list:itemFormat|spacer}` renders every item of a list with the same
//! format and writes the spacer between them. A third part is the spacer before
//! the last item, a fourth the spacer for a list of exactly two:
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! // Registered by default, at index 0 — where .NET sorts it.
//! let smart = SmartFormatter::default();
//!
//! let names = Value::List(vec![Value::from("Jim"), Value::from("Pam"), Value::from("Dwight")]);
//! let args = Value::List(vec![names]);
//! assert_eq!(
//!     smart.format("{0:list:{}|, |, and }", &args).unwrap(),
//!     "Jim, Pam, and Dwight"
//! );
//! ```

use std::borrow::Cow;

use crate::error::Error;
use crate::formatter::{Formatter, FormattingInfo, NO_COLLECTION_INDEX};
use crate::parsing::chars::NULLABLE_OPERATOR;
use crate::parsing::{Format, FormatItem, Placeholder, SplitPiece};
use crate::value::Value;

use super::{split_part, InvalidSplitChar, DEFAULT_SPLIT_CHAR};

/// The default formatter name, .NET `ListFormatter.Name`.
const NAME: &str = "list";

/// The number of separators .NET asks `Format.Split` for, one less than the
/// parts it reads: an item format, a spacer, a last spacer and a two spacer.
const MAX_SEPARATORS: usize = 4;

/// Renders each item of a list with the same format, ported from .NET
/// `ListFormatter`.
///
/// The formatter auto-detects, and .NET ranks it before every other formatter,
/// so `{0:one|many}` on a list is a list — item format `one`, spacer `many` —
/// rather than a plural. Registering it anywhere but at index 0 changes that.
#[derive(Debug, Clone)]
pub struct ListFormatter {
    name: String,
    split_char: char,
    can_auto_detect: bool,
}

impl ListFormatter {
    /// A formatter named `list`, splitting on `|` and auto-detecting — the
    /// .NET defaults.
    pub fn new() -> Self {
        Self {
            name: NAME.to_owned(),
            split_char: DEFAULT_SPLIT_CHAR,
            can_auto_detect: true,
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The character the parts of the format are split on.
    pub fn split_char(&self) -> char {
        self.split_char
    }

    /// Changes the split character, so that the character it replaces can be
    /// used in the output: with `~`, `{0:list:{}~|~|}` writes `one|two|three`.
    ///
    /// Only the characters in [`VALID_SPLIT_CHARS`](super::VALID_SPLIT_CHARS)
    /// are accepted; .NET throws an `ArgumentException` for anything else.
    pub fn set_split_char(&mut self, split_char: char) -> Result<(), InvalidSplitChar> {
        self.split_char = super::valid_split_char(split_char)?;
        Ok(())
    }

    /// Whether a placeholder that names no formatter may be handled here,
    /// .NET's settable `CanAutoDetect`, which defaults to `true`.
    pub fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    pub fn set_can_auto_detect(&mut self, can_auto_detect: bool) {
        self.can_auto_detect = can_auto_detect;
    }

    /// .NET `ListFormatter.FormatItems`: every item, with a spacer between.
    fn format_items(
        &self,
        info: &mut FormattingInfo<'_>,
        items: &[Value],
        parts: &[SplitPiece],
        item_format: &Format,
    ) -> Result<(), Error> {
        // .NET cuts all three spacers out of the format here, before it writes
        // anything, so a piece that cannot be cut fails the whole list and not
        // just the item that would have used it.
        let spacer = split_part(info, &parts[1])?;
        let last_spacer = match parts.get(2) {
            Some(part) => split_part(info, part)?,
            None => spacer,
        };
        let two_spacer = match parts.get(3) {
            Some(part) => split_part(info, part)?,
            None => last_spacer,
        };

        // The spacers are formatted against the value the call was made with,
        // not against an item: inside a list the current value is an item, and
        // a spacer like `{Split}` means something in the caller's data.
        let root = info.root_value();
        let count = i32::try_from(items.len()).unwrap_or(i32::MAX);

        for item in items {
            // .NET counts in the shared collection index rather than in a local
            // of the loop, which is what makes `{Index}` work.
            let index = info.collection_index() + 1;
            info.set_collection_index(index);

            let spacer = if index == 0 {
                None // Nothing goes before the first item.
            } else if index < count - 1 {
                Some(spacer)
            } else if index == 1 {
                Some(two_spacer) // The second item is also the last one.
            } else {
                Some(last_spacer)
            };

            if let Some(spacer) = spacer {
                write_spacer(info, spacer, root)?;
            }

            let alignment = info.alignment();
            info.format_as_child_of_current(item_format, item, alignment)?;
        }

        Ok(())
    }
}

impl Default for ListFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter for ListFormatter {
    /// Itself, so that a caller who registered it can find it again through
    /// [`FormatterRegistry::get_mut`](crate::formatter::FormatterRegistry::get_mut)
    /// and set its knobs.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn can_auto_detect(&self) -> bool {
        self.can_auto_detect
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let format = info.format();
        let current = info.current();

        // Called by name on a null value, the nullable operator makes the
        // placeholder empty instead of an error: `{TheList?:list:{}|, }`.
        if matches!(current, Value::Null) && has_nullable_operator(info) {
            info.write("");
            return Ok(true);
        }

        // .NET splits before it counts the parts, so a split that fails is the
        // answer however many parts the formatter wanted.
        let parts = format
            .map(|format| {
                format
                    .split_max(self.split_char, MAX_SEPARATORS)
                    .map_err(|error| info.plain_error_here(&error.to_string()))
            })
            .transpose()?;

        // Check whether the arguments can be handled by this formatter. A
        // string is an `IEnumerable` in .NET and excluded here all the same;
        // so is anything `IFormattable`.
        let (parts, items) = match (parts, current) {
            (Some(parts), Value::List(items)) if parts.len() >= 2 => (parts, items),
            _ => {
                return info.decline_or_error(|name| {
                    format!(
                        "Formatter named '{name}' requires an IEnumerable argument and at least 2 format parameters."
                    )
                })
            }
        };

        // The item format is either a nested format, evaluated against each
        // item, or a format specifier — which is the same thing once it is
        // wrapped in a placeholder of its own.
        let item_format = split_part(info, &parts[0])?;
        let wrapped;
        let item_format = if item_format.has_nested() {
            item_format
        } else {
            wrapped = as_placeholder(item_format, info.alignment());
            &wrapped
        };

        // A list nested in a list has to leave the enclosing index alone.
        let saved = info.collection_index();
        info.set_collection_index(NO_COLLECTION_INDEX);

        self.format_items(info, items, &parts, item_format)?;

        // Restored only on the way out without an error, as in .NET, which has
        // no `finally` here: an error the settings recover from — a spacer
        // whose escape sequence does not resolve, say — leaves the index where
        // the loop got to, and a later `{Index}` in the same call reads it.
        info.set_collection_index(saved);

        Ok(true)
    }
}

/// Whether any selector of the placeholder carries the nullable operator
/// (.NET `ListFormatter.HasNullableOperator`).
///
/// Not [`SelectorInfo::has_nullable_operator`](crate::sources::SelectorInfo::has_nullable_operator):
/// .NET's copy here counts a bare `?`, where the one in `Source` wants the `?.`
/// or `?[` form. `{TheList?:list:…}` has nothing after the `?` to take, so only
/// this reading of it makes the nullable list of the .NET tests work.
fn has_nullable_operator(info: &FormattingInfo<'_>) -> bool {
    info.placeholder()
        .selectors
        .iter()
        .any(|selector| selector.operator.starts_with(NULLABLE_OPERATOR))
}

/// The spacer between two items: a format with placeholders is evaluated
/// against `value`, one without is written as it stands
/// (.NET `ListFormatter.WriteSpacer`).
fn write_spacer(
    info: &mut FormattingInfo<'_>,
    spacer: &Format,
    value: &Value,
) -> Result<(), Error> {
    if spacer.has_nested() {
        // A spacer does not inherit the alignment of the placeholder, so
        // `{0,5:list:{}|-}` pads the items and not the `-` between them. A
        // placeholder *inside* the spacer keeps the alignment the parser gave
        // it, which is the enclosing one — `{0,5:list:{}|{1}}` pads both.
        return info.format_as_child_of_current(spacer, value, 0);
    }

    let text = literal_text(info, spacer)?;
    info.write_unaligned(&text);
    Ok(())
}

/// The literal text of a format (.NET `Format.GetLiteralText`), which resolves
/// the escape sequences and throws where one resolves to nothing.
///
/// A spacer is written once per gap, and the ordinary one — `, ` — is a single
/// literal whose resolved text is already the answer, so that case borrows
/// rather than building the string again per gap.
fn literal_text<'f>(info: &FormattingInfo<'_>, format: &'f Format) -> Result<Cow<'f, str>, Error> {
    if let Some((message, _)) = format.first_escape_error() {
        return Err(info.plain_error_here(message));
    }
    if let [FormatItem::Literal(literal)] = format.items.as_slice() {
        return Ok(Cow::Borrowed(&literal.text));
    }
    Ok(Cow::Owned(format.literal_text()))
}

/// A format holding one placeholder that has no selectors and `format` as its
/// format, which is how .NET turns an item format that is a *specifier* —
/// `{0:list:N2|, }` — into something it can evaluate per item: the placeholder
/// resolves to the item, and hands `N2` to whichever formatter takes it.
fn as_placeholder(format: &Format, alignment: i32) -> Format {
    let mut placeholder = Placeholder {
        selectors: Vec::new(),
        // .NET inherits the alignment of the placeholder being formatted.
        alignment,
        format: Some(format.clone()),
        start: format.start,
        end: format.end,
        ..Placeholder::default()
    };
    // What .NET's `Placeholder.RawText` rebuilds for it.
    placeholder.raw = placeholder.to_string();

    Format {
        raw: format.raw.clone(),
        items: vec![FormatItem::Placeholder(placeholder)],
        start: format.start,
        end: format.end,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/ListFormatterTests.cs`, with the cases
    //! that need extensions we do not have adapted: `ToCharArray` (a
    //! `StringSource` selector with no `Value` to return) becomes a list
    //! written out, and reflection over anonymous types becomes a map. The
    //! thread-safety cases have no counterpart — the collection index belongs
    //! to the format call here, so there is nothing to mix up.
    //!
    //! The formatter is registered by hand, since the default registry has no
    //! slot for it yet; the source half is registered by default already.

    use std::collections::BTreeMap;

    use super::*;
    use crate::extensions::envelope;
    use crate::extensions::ConditionalFormatter;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::parsing::SplitError;
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::SmartFormatter;

    /// The formatter under test, the always-last [`DefaultFormatter`], and
    /// nothing else — so these tests pin this formatter rather than the order
    /// of the default registry.
    fn smart_with(settings: SmartSettings, formatter: ListFormatter) -> SmartFormatter {
        let mut smart = SmartFormatter::new(settings);
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(formatter));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    /// The .NET default of `FormatErrorAction.ThrowError` — our
    /// [`ErrorAction::Error`] — so a formatting error surfaces in the tests.
    fn smart() -> SmartFormatter {
        smart_with(throwing(), ListFormatter::new())
    }

    fn throwing() -> SmartSettings {
        SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        }
    }

    fn list(values: impl IntoIterator<Item = Value>) -> Value {
        Value::List(values.into_iter().collect())
    }

    fn strings(values: impl IntoIterator<Item = &'static str>) -> Value {
        list(values.into_iter().map(Value::from))
    }

    fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    /// The arguments of .NET's `ListFormatterTests.GetArgs`, as far as they
    /// have a `Value`: the characters of "ABCDE", five words, three people
    /// with friends, three dates (as text, since the case that uses them hands
    /// them to the default formatter) and five numbers.
    fn args() -> Value {
        let person = |name: &str, friends: &[&str]| {
            map([
                ("FirstName", Value::from(name)),
                (
                    "Friends",
                    list(
                        friends
                            .iter()
                            .map(|friend| map([("FirstName", Value::from(*friend))])),
                    ),
                ),
            ])
        };

        list([
            strings(["A", "B", "C", "D", "E"]),
            strings(["One", "Two", "Three", "Four", "Five"]),
            list([
                person("Jim", &["Dwight", "Michael"]),
                person("Pam", &["Dwight", "Michael"]),
                person("Dwight", &["Michael"]),
            ]),
            strings(["1/1/2000", "10/10/2010", "5/5/5555"]),
            list((1..=5).map(Value::Int)),
        ])
    }

    fn format(template: &str) -> String {
        smart()
            .format(template, &args())
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    fn error_of(smart: &SmartFormatter, template: &str, args: &Value) -> String {
        match smart.format(template, args) {
            Err(Error::Format { message, .. }) => message,
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    /// .NET's `FormattingException.Message`, which
    /// [`ErrorAction::OutputErrorInResult`] writes into the result verbatim.
    /// What .NET throws when the formatter is named and cannot take the value.
    const NOT_A_LIST: &str =
        "Formatter named 'list' requires an IEnumerable argument and at least 2 format parameters.";

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = ListFormatter::new();
        assert_eq!(formatter.name(), "list");
        assert!(formatter.can_auto_detect());
        assert_eq!(formatter.split_char(), '|');
        assert_eq!(ListFormatter::new().with_name("l").name(), "l");
    }

    #[test]
    fn rejects_an_invalid_split_char() {
        let mut formatter = ListFormatter::new();
        assert_eq!(formatter.set_split_char('/'), Err(InvalidSplitChar('/')));
        assert_eq!(formatter.split_char(), '|');
        assert!(formatter.set_split_char('~').is_ok());
        assert_eq!(formatter.split_char(), '~');
    }

    #[test]
    fn simple_list() {
        let args = list([strings(["one", "two", "three"])]);
        let result = smart().format("{0:list:{}|, |, and }", &args).unwrap();
        assert_eq!(result, "one, two, and three");
    }

    #[test]
    fn a_changed_split_char_frees_the_pipe() {
        let mut formatter = ListFormatter::new();
        formatter.set_split_char('~').unwrap();
        let smart = smart_with(throwing(), formatter);
        let args = list([strings(["one", "two", "three"])]);
        let result = smart.format("{0:list:{}~|~|}", &args).unwrap();
        assert_eq!(result, "one|two|three");
    }

    #[test]
    fn an_empty_list_writes_nothing() {
        let args = list([Value::List(Vec::new())]);
        let result = smart().format(">{0:list:{}|, |, and }<", &args).unwrap();
        assert_eq!(result, "><");
    }

    #[test]
    fn a_null_list_is_empty_under_the_nullable_operator() {
        let data = map([("TheList", Value::Null)]);
        let result = smart()
            .format("{TheList?:list:{}|, |, and }", &data)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn a_null_list_without_the_nullable_operator_is_an_error() {
        let data = map([("TheList", Value::Null)]);
        assert_eq!(
            error_of(&smart(), "{TheList:list:{}|, |, and }", &data),
            NOT_A_LIST
        );
    }

    #[test]
    fn the_item_format_may_be_a_specifier() {
        // .NET wraps a format that holds no placeholder in a placeholder of its
        // own, so each item is formatted with it as the specifier.
        assert_eq!(format("{4:list:|}"), "12345");
        // .NET's case spells this `{4:list:00|}`; a custom numeric pattern is a
        // non-goal of the port (see DESIGN.md), so the standard `D2` stands in.
        assert_eq!(format("{4:list:D2|}"), "0102030405");
        assert_eq!(format("{4:list:|,}"), "1,2,3,4,5");
        assert_eq!(format("{4:list:|, |, and }"), "1, 2, 3, 4, and 5");
        assert_eq!(
            format("{4:list:N2|, |, and }"),
            "1.00, 2.00, 3.00, 4.00, and 5.00"
        );
        // And the custom pattern fails as it does anywhere else.
        assert!(matches!(
            smart().format("{4:list:00|}", &args()),
            Err(Error::UnsupportedSpec { .. })
        ));
    }

    #[test]
    fn the_item_format_may_hold_placeholders() {
        assert_eq!(format("{0:list:{}-|}"), "A-B-C-D-E-");
        assert_eq!(format("{0:list:{}|-}"), "A-B-C-D-E");
        assert_eq!(format("{0:list:{}|-|+}"), "A-B-C-D+E");
        assert_eq!(
            format("{0:list:({})|, |, and }"),
            "(A), (B), (C), (D), and (E)"
        );
    }

    #[test]
    fn the_spacers_depend_on_the_number_of_parts_and_of_items() {
        let three = list([strings(["a", "b", "c"])]);
        let two = list([strings(["a", "b"])]);
        let one = list([strings(["a"])]);
        let smart = smart();
        let format = |template: &str, args: &Value| smart.format(template, args).unwrap();

        // Two parts: the one spacer goes between all of them.
        assert_eq!(format("{0:list:{}|, }", &three), "a, b, c");
        assert_eq!(format("{0:list:{}|, }", &two), "a, b");
        // Three: the third part is the spacer before the last item, which for a
        // list of two is the only spacer there is.
        assert_eq!(format("{0:list:{}|, |, and }", &three), "a, b, and c");
        assert_eq!(format("{0:list:{}|, |, and }", &two), "a, and b");
        // Four: the fourth part replaces it when the list holds exactly two.
        assert_eq!(format("{0:list:{}|, |, and | & }", &three), "a, b, and c");
        assert_eq!(format("{0:list:{}|, |, and | & }", &two), "a & b");
        assert_eq!(format("{0:list:{}|, |, and | & }", &one), "a");
        // A fifth part is split off and then ignored.
        assert_eq!(format("{0:list:{}|1|2|3|4}", &three), "a1b2c");
    }

    #[test]
    fn a_spacer_is_formatted_against_the_data_of_the_call() {
        // The current value inside a list is an item, so a placeholder in a
        // spacer would find nothing there; .NET hands it the value the call was
        // made with instead.
        let data = map([
            ("Names", strings(["John", "Mary", "Amy"])),
            ("Split", Value::from(", ")),
            ("IsAnd", Value::Bool(true)),
        ]);

        let mut smart = smart();
        smart
            .formatters_mut()
            .insert(1, Box::new(ConditionalFormatter::new()));
        let result = smart
            .format("{Names:list:{}|{Split}| {IsAnd:and|nor} }", &data)
            .unwrap();
        assert_eq!(result, "John, Mary and Amy");

        // The same template with positional arguments.
        let args = list([
            strings(["John", "Mary", "Amy"]),
            Value::from(", "),
            Value::from("and"),
        ]);
        assert_eq!(
            smart.format("{0:list:{}|{1}| {2} }", &args).unwrap(),
            "John, Mary and Amy"
        );
    }

    #[test]
    fn an_item_format_falls_back_to_the_enclosing_scopes() {
        let data = map([
            ("Names", strings(["John", "Mary"])),
            ("Split", Value::from("+")),
        ]);
        let result = smart().format("{Names:list:{}{Split}|, }", &data).unwrap();
        assert_eq!(result, "John+, Mary+");
    }

    #[test]
    fn nested_lists() {
        assert_eq!(format("{2:list:{:{FirstName}}|, }"), "Jim, Pam, Dwight");
        assert_eq!(
            format("{2:list:{:{FirstName}'s friends: {Friends:list:{FirstName}|, }}|; }"),
            "Jim's friends: Dwight, Michael; Pam's friends: Dwight, Michael; Dwight's friends: Michael"
        );
    }

    #[test]
    fn a_list_of_lists_with_an_element_format() {
        let data = list([list([
            list((1..=3).map(Value::Int)),
            list((4..=6).map(Value::Int)),
            list((7..=9).map(Value::Int)),
        ])]);
        // .NET's element format is the custom pattern `000`, which the port
        // does not render; `D3` is the standard specifier for the same thing.
        let result = smart()
            .format("{0:list:{:list:{:D3}|, |, }|\n|\n}", &data)
            .unwrap();
        assert_eq!(result, "001, 002, 003\n004, 005, 006\n007, 008, 009");
    }

    #[test]
    fn index_is_the_index_of_the_item() {
        assert_eq!(
            format("{0:list:{} = {Index}|, }"),
            "A = 0, B = 1, C = 2, D = 3, E = 4"
        );
    }

    #[test]
    fn index_is_the_index_of_the_innermost_list() {
        // .NET's case uses `ToCharArray`, which has no `Value` to return; a
        // list of lists puts the same question.
        let data = list([list([strings(["O", "n", "e"]), strings(["T", "w", "o"])])]);
        let result = smart()
            .format("{0:list:{Index}: {:list:{} = {Index}|, }|; }", &data)
            .unwrap();
        assert_eq!(result, "0: O = 0, n = 1, e = 2; 1: T = 0, w = 1, o = 2");
    }

    #[test]
    fn index_synchronizes_two_lists() {
        assert_eq!(
            format("{0:list:{} = {1.Index}|, }"),
            "A = One, B = Two, C = Three, D = Four, E = Five"
        );
        assert_eq!(
            format("{0:list:{} = {1[Index]}|, }"),
            "A = One, B = Two, C = Three, D = Four, E = Five"
        );
        // The selector is matched ignoring case, whatever the settings say.
        assert_eq!(
            format("{0:list:{} = {1.INDEX}|, }"),
            "A = One, B = Two, C = Three, D = Four, E = Five"
        );
    }

    #[test]
    fn a_second_list_too_short_to_synchronize_falls_back_to_the_scopes() {
        // With the index out of range the source declines, and the selector
        // reaches the list being iterated instead, whose `Index` is in range —
        // which is what .NET answers (probed).
        let args = list([list((1..=3).map(Value::Int)), strings(["x"])]);
        let result = smart().format("{0:list:{}={1.Index}|, }", &args).unwrap();
        assert_eq!(result, "1=x, 2=2, 3=3");
    }

    #[test]
    fn index_outside_a_list_is_minus_one() {
        assert_eq!(format("{Index}"), "-1");
        // Any enumerable answers, a string and a map included.
        let smart = smart();
        assert_eq!(
            smart.format("{Index}", &Value::from("hello")).unwrap(),
            "-1"
        );
        assert_eq!(
            smart
                .format("{Index}", &map([("a", Value::Int(1))]))
                .unwrap(),
            "-1"
        );
        // A value that is no enumerable at all is not answered at all.
        assert_eq!(
            error_of(&smart, "{Index}", &list([Value::Int(5)])),
            envelope(
                "{Index}",
                "No source extension could handle the selector named \"Index\"",
                1
            )
        );
        // Neither is `Index` deeper in a selector chain.
        assert_eq!(
            error_of(&smart, "{0.Index}", &list([strings(["a"])])),
            envelope(
                "{0.Index}",
                "No source extension could handle the selector named \"Index\"",
                3
            )
        );
    }

    #[test]
    fn index_is_restored_after_a_nested_list() {
        let args = list([list((1..=3).map(Value::Int)), strings(["x", "y"])]);
        let result = smart()
            .format("{0:list:[{Index}:{1:list:{}|,}:{Index}]|;}", &args)
            .unwrap();
        assert_eq!(result, "[0:x,y:0];[1:x,y:1];[2:x,y:2]");
    }

    #[test]
    fn index_is_minus_one_again_after_the_list() {
        let args = list([strings(["a", "b"])]);
        let result = smart().format("{0:list:{}|,}[{Index}]", &args).unwrap();
        assert_eq!(result, "a,b[-1]");
    }

    #[test]
    fn a_numeric_selector_indexes_a_list() {
        let data = map([("Numbers", strings(["dummy", "one"]))]);
        let smart = smart();
        assert_eq!(smart.format(">{Numbers.1}<", &data).unwrap(), ">one<");
        assert_eq!(smart.format(">{Numbers[1]}<", &data).unwrap(), ">one<");

        let data = map([("Numbers", list([Value::from("dummy"), Value::Null]))]);
        assert_eq!(smart.format(">{Numbers.1}<", &data).unwrap(), "><");
        assert_eq!(smart.format(">{Numbers[1]}<", &data).unwrap(), "><");
    }

    #[test]
    fn a_null_list_indexed_under_the_nullable_operator_is_empty() {
        let data = map([("Numbers", Value::Null)]);
        let smart = smart();
        assert_eq!(smart.format(">{Numbers?.0}<", &data).unwrap(), "><");
        assert_eq!(smart.format(">{Numbers?[0]}<", &data).unwrap(), "><");
    }

    #[test]
    fn a_null_list_indexed_without_it_is_an_error() {
        let data = map([("Numbers", Value::Null)]);
        let smart = smart();
        for template in [">{Numbers.0}<", ">{Numbers[0]}<"] {
            assert!(
                error_of(&smart, template, &data).contains("the selector named \"0\""),
                "{template}"
            );
        }
    }

    #[test]
    fn a_value_that_is_no_list_is_an_error_when_the_formatter_is_named() {
        let smart = smart();
        for args in [
            list([Value::from("not a list")]),
            list([Value::Int(42)]),
            // A map is an `IEnumerable` of pairs in .NET, which renders
            // `[a, 1], [b, 2]`; there is nothing there worth reproducing.
            list([map([("a", Value::Int(1))])]),
        ] {
            assert_eq!(error_of(&smart, "{0:list:{}|, |, and }", &args), NOT_A_LIST);
        }
        // Fewer than two parts is the same error.
        assert_eq!(
            error_of(&smart, "{0:list:{}}", &list([strings(["a"])])),
            NOT_A_LIST
        );
    }

    #[test]
    fn a_value_that_is_no_list_is_declined_when_it_is_not() {
        // Auto-detection reports a failure to evaluate, and the next formatter
        // — here the default one — takes the value.
        let smart = smart();
        assert_eq!(
            smart
                .format("{0:one|many}", &list([Value::from("x")]))
                .unwrap(),
            "x"
        );
        // Fewer than two parts is declined as well, however enumerable the
        // value is.
        assert_eq!(
            smart.format("{0:one}", &list([Value::from("x")])).unwrap(),
            "x"
        );
    }

    #[test]
    fn a_list_is_auto_detected() {
        // .NET ranks the list formatter before every other one, so a
        // `|`-separated format on a list is a list: the first part formats each
        // item, the second is the spacer.
        let args = list([strings(["a", "b", "c"])]);
        let smart = smart();
        assert_eq!(smart.format("{0:one|many}", &args).unwrap(), "amanybmanyc");
        assert_eq!(
            smart.format("{0:one|many|last}", &args).unwrap(),
            "amanyblastc"
        );
        assert_eq!(smart.format("{0:{}|many}", &args).unwrap(), "amanybmanyc");

        // Turning auto-detection off leaves the format to the next formatter,
        // which here is the default one and has no answer for a list.
        let mut formatter = ListFormatter::new();
        formatter.set_can_auto_detect(false);
        assert!(!formatter.can_auto_detect());
        let smart = smart_with(throwing(), formatter);
        assert!(smart.format("{0:one|many}", &args).is_err());
    }

    #[test]
    fn the_index_selector_is_recognized_without_a_formatter_name() {
        // .NET's `Enumerable_With_SelectorName_Index_Is_Recognized`.
        let items = list([
            map([("Content", Value::from("Content A"))]),
            map([("Content", Value::from("Content B"))]),
        ]);
        let result = smart()
            .format("{0:{Content} with Index {Index}|, }", &list([items]))
            .unwrap();
        assert_eq!(result, "Content A with Index 0, Content B with Index 1");
    }

    #[test]
    fn the_alignment_applies_to_the_items_and_not_to_the_spacers() {
        let args = list([strings(["a", "b", "c"])]);
        let smart = smart();
        assert_eq!(
            smart.format(">{0,5:list:{}|-}<", &args).unwrap(),
            ">    a-    b-    c<"
        );
        assert_eq!(
            smart.format(">{0,-3:list:{}|-}<", &args).unwrap(),
            ">a  -b  -c  <"
        );
        // A specifier as the item format inherits the alignment too.
        let numbers = list([list([Value::Int(1), Value::Int(2)])]);
        assert_eq!(
            smart.format(">{0,5:list:N2|, }<", &numbers).unwrap(),
            "> 1.00,  2.00<"
        );
        // A placeholder *inside* a spacer keeps the alignment the parser gave
        // it, which is the enclosing placeholder's.
        let args = list([strings(["a", "b"]), Value::from("+")]);
        assert_eq!(
            smart.format(">{0,5:list:{}|{1}}<", &args).unwrap(),
            ">    a    +    b<"
        );
    }

    #[test]
    fn the_search_for_the_separators_stops_after_the_fourth() {
        // .NET asks for at most four separators, so a literal whose ends the
        // parser crossed — here the invalid `\u12` — is never reached when it
        // sits in the fifth part. An unlimited split would fail the whole
        // placeholder instead (probed against 3.6.1).
        let args = list([strings(["a", "b", "c"])]);
        let smart = smart();
        assert_eq!(
            smart.format(r"{0:list:{}|-|+|*|x\u12}", &args).unwrap(),
            "a-b+c"
        );
        assert_eq!(
            smart.format(r"{0:list:{}|-|+|x\u12|z}", &args).unwrap(),
            "a-b+c"
        );
        // Reached while the search is still going, it fails the placeholder.
        assert_eq!(
            error_of(&smart, r"{0:list:{}|x\u12}", &args),
            SplitError::Count.message()
        );
    }

    #[test]
    fn a_spacer_whose_escape_sequence_does_not_resolve_fails_where_it_is_written() {
        let args = list([strings(["a", "b", "c"])]);
        assert_eq!(
            error_of(&smart(), r">{0:list:{}|a\qb}<", &args),
            "Unrecognized escape sequence \"\\q\" in literal."
        );
        // What was written before the error survives the recovery.
        let smart = smart_with(
            SmartSettings {
                format_error_action: ErrorAction::OutputErrorInResult,
                ..SmartSettings::default()
            },
            ListFormatter::new(),
        );
        assert_eq!(
            smart.format(r">{0:list:{}|a\qb}<", &args).unwrap(),
            ">aUnrecognized escape sequence \"\\q\" in literal.<"
        );
    }

    #[test]
    fn an_escape_sequence_in_a_spacer_is_resolved() {
        let args = list([strings(["a", "b"])]);
        assert_eq!(smart().format(r"{0:list:{}|\n}", &args).unwrap(), "a\nb");
    }

    #[test]
    fn default_formatting_of_a_list_is_still_an_error() {
        // .NET renders `{4:list}` as `System.Int32[]`: with no format at all the
        // list formatter declines, and the default formatter writes the CLR
        // type name. We fail loudly instead — see DESIGN.md.
        assert!(smart().format("{4:list}", &args()).is_err());
    }
}
