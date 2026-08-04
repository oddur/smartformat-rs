//! Port of SmartFormat.NET's `TemplateFormatter`.
//!
//! Ported from `src/SmartFormat/Extensions/TemplateFormatter.cs`.
//!
//! `{:t:firstLast}` renders a template that was registered under the name
//! `firstLast` against the current value. The name comes either from the
//! formatter options — `{:t(firstLast)}` — or, when there are none, from the
//! placeholder's format.
//!
//! .NET's `CreateDefaultSmartFormat` does not register this formatter, so a
//! [`SmartFormatter`](crate::SmartFormatter) grows one the first time it is
//! handed a template. Registering through
//! [`SmartFormatter::register_template`](crate::SmartFormatter::register_template)
//! is the way in; the type's own [`register`](TemplateFormatter::register) is
//! for a caller assembling a registry by hand, and needs the parser passed
//! explicitly — as [`TemplateFormatter::new`] needs the host's
//! [`CaseSensitivity`], which .NET's `Initialize` copies off the
//! `SmartFormatter` the extension is added to.
//!
//! ```
//! use smartformat::{SmartFormatter, Value};
//!
//! let mut smart = SmartFormatter::default();
//! smart.register_template("firstLast", "{First} {Last}").unwrap();
//!
//! let person = Value::Map(
//!     [
//!         ("First".to_owned(), Value::from("Scott")),
//!         ("Last".to_owned(), Value::from("Rippey")),
//!     ]
//!     .into_iter()
//!     .collect(),
//! );
//! assert_eq!(smart.format("{:t:firstLast}", &person).unwrap(), "Scott Rippey");
//! assert_eq!(smart.format("{:t(firstLast)}", &person).unwrap(), "Scott Rippey");
//! ```

use std::fmt;

use crate::formatter::{Formatter, FormattingInfo};
use crate::parsing::{Format, Parser};
use crate::settings::CaseSensitivity;
use crate::Error;

/// The default formatter name, .NET `TemplateFormatter.Name`.
///
/// .NET 3.6.1 still carries an obsolete `Names` array holding `template` and
/// `t`, but only `Name` selects a formatter, so `{:template:firstLast}` is
/// "No suitable Formatter could be found" there and here (probed).
const NAME: &str = "t";

/// The .NET `ArgumentNullException` from `Dictionary.TryGetValue(null)`, which
/// a placeholder with a formatter name but no format at all would reach; see
/// [`TemplateFormatter::template_name`].
const NULL_KEY: &str = "Value cannot be null. (Parameter 'key')";

/// Renders a template registered under a name, ported from .NET
/// `TemplateFormatter`.
///
/// The formatter owns the registry of templates: [`register`](Self::register)
/// parses a template *once* and keeps the parsed [`Format`], so rendering it
/// costs no parsing. .NET's `Register` parses with the owning
/// `SmartFormatter`'s parser, which it gets from `IInitializer.Initialize`; a
/// [`Formatter`] here cannot hold that reference, so the parser is passed in
/// instead — hand it [`SmartFormatter::parser`](crate::SmartFormatter::parser)
/// so that a template and the templates' host agree on the syntax.
///
/// A placeholder always has to name the formatter: .NET's `CanAutoDetect`
/// throws when it is set to `true`, so
/// [`can_auto_detect`](Formatter::can_auto_detect) is `false` and cannot be
/// changed.
#[derive(Debug, Clone)]
pub struct TemplateFormatter {
    name: String,
    case_sensitivity: CaseSensitivity,
    /// The registered templates, in registration order.
    ///
    /// .NET keeps a `Dictionary<string, Format>` built with the owning
    /// formatter's `SmartSettings.GetCaseSensitivityComparer()`. A `HashMap`
    /// cannot switch its comparer at run time, and template registries are
    /// small, so this is a list scanned with the same [`CaseSensitivity::eq`]
    /// every other name comparison in this crate uses.
    templates: Vec<(String, Format)>,
}

impl TemplateFormatter {
    /// A formatter named `t` whose template names are matched with
    /// `case_sensitivity`.
    ///
    /// This is .NET's `Initialize`, which builds the dictionary with the
    /// comparer of the `SmartFormatter` the formatter is added to, so pass
    /// [`SmartSettings::case_sensitive`](crate::SmartSettings::case_sensitive)
    /// of the formatter this one is registered with. As in .NET, the choice is
    /// then fixed: a [`SmartFormatter`](crate::SmartFormatter) whose setting is
    /// changed afterwards does not change how template names are matched here.
    ///
    /// There is deliberately no `Default` and no argument-less constructor.
    /// .NET's `AddExtensions` always runs `Initialize`, so the host's setting
    /// always wins there however the extension was built; a default here would
    /// be silently wrong for a host set to
    /// [`CaseSensitivity::CaseInsensitive`], which is the one thing this
    /// type cannot recover from later.
    pub fn new(case_sensitivity: CaseSensitivity) -> Self {
        Self {
            name: NAME.to_owned(),
            case_sensitivity,
            templates: Vec::new(),
        }
    }

    /// Renames the formatter, as .NET's settable `IFormatter.Name` does.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// How template names are matched, .NET's dictionary comparer.
    pub fn case_sensitivity(&self) -> CaseSensitivity {
        self.case_sensitivity
    }

    /// Parses `template` and registers it under `name` (.NET `Register`).
    ///
    /// `parser` is the parser of the [`SmartFormatter`](crate::SmartFormatter)
    /// that renders the templates — .NET reaches it through
    /// `_formatter.Parser`, which it is handed in `Initialize`.
    ///
    /// Fails if `template` does not parse, or if `name` is already registered:
    /// .NET's `Dictionary.Add` throws an `ArgumentException` rather than
    /// overwriting, so re-registering a name means [`remove`](Self::remove)
    /// first.
    ///
    /// The two checks run in .NET's order — `Register` is
    /// `var parsed = Parser.ParseFormat(template); _templates.Add(name, parsed);`
    /// — so a duplicate name carrying a template that does not parse reports
    /// the *parse* error, not the duplicate.
    pub fn register(
        &mut self,
        parser: &Parser,
        name: impl Into<String>,
        template: &str,
    ) -> Result<(), RegisterError> {
        let name = name.into();
        // .NET parses eagerly, so a template that does not parse fails here
        // rather than when a placeholder asks for it — and before the
        // dictionary is touched at all.
        let parsed = parser.parse(template).map_err(RegisterError::Parse)?;
        if self.get(&name).is_some() {
            return Err(RegisterError::Duplicate(name));
        }
        self.templates.push((name, parsed));
        Ok(())
    }

    /// The template registered under `name`, matched with this formatter's
    /// [`case_sensitivity`](Self::case_sensitivity).
    pub fn get(&self, name: &str) -> Option<&Format> {
        self.templates
            .iter()
            .find(|(registered, _)| self.case_sensitivity.eq(registered, name))
            .map(|(_, template)| template)
    }

    /// Removes a template, reporting whether there was one (.NET `Remove`).
    pub fn remove(&mut self, name: &str) -> bool {
        let case_sensitivity = self.case_sensitivity;
        let before = self.templates.len();
        self.templates
            .retain(|(registered, _)| !case_sensitivity.eq(registered, name));
        self.templates.len() != before
    }

    /// Removes every template (.NET `Clear`).
    pub fn clear(&mut self) {
        self.templates.clear();
    }

    /// How many templates are registered.
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// The name of the template a placeholder asks for, or `None` when this
    /// formatter declines the placeholder.
    ///
    /// .NET takes the name from `FormatterOptions` — `{:t(firstLast)}` — and
    /// falls back to `Format.RawText` when there are none. `RawText` is the
    /// format's text with escape sequences *resolved*, so `{:t:\{x\}}` asks
    /// for the template named `{x}`, and `{:t:a\q}` fails the way writing that
    /// literal would (probed).
    ///
    /// A format holding a placeholder is declined instead — `{:t:{First}}` is
    /// not a template name — which leaves the placeholder to be reported as
    /// "No suitable Formatter could be found", since a named formatter that
    /// declines and a missing one are reported alike.
    fn template_name(&self, info: &FormattingInfo<'_>) -> Result<Option<String>, Error> {
        // .NET reads `FormatterOptions`, which resolves escape sequences and
        // throws when one resolves to nothing — outside the evaluator's error
        // handling, so such a placeholder fails the call whatever the error
        // action is.
        let options = info.formatter_options()?;
        if !options.is_empty() {
            return Ok(Some(options.to_owned()));
        }

        match info.format() {
            Some(format) if format.has_nested() => Ok(None),
            Some(format) => raw_text(info, format).map(Some),
            // Unreachable through the parser: a placeholder that names a
            // formatter always has a format, empty as it may be — `{:t()}`
            // carries one of length zero. .NET would hand the `null` to
            // `Dictionary.TryGetValue` and get an `ArgumentNullException`.
            None => Err(info.plain_error_here(NULL_KEY)),
        }
    }
}

impl Formatter for TemplateFormatter {
    /// Itself, which is what lets
    /// [`SmartFormatter::register_template`](crate::SmartFormatter::register_template)
    /// add a template to a formatter the registry already owns.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn name(&self) -> &str {
        &self.name
    }

    /// Never: .NET's `CanAutoDetect` setter throws for `true`
    /// ("TemplateFormatter cannot handle auto-detection").
    fn can_auto_detect(&self) -> bool {
        false
    }

    fn try_evaluate_format(&self, info: &mut FormattingInfo<'_>) -> Result<bool, Error> {
        let Some(name) = self.template_name(info)? else {
            return Ok(false);
        };

        let Some(template) = self.get(&name) else {
            // .NET throws a plain `FormatException`, which the evaluator
            // catches: `ErrorAction::OutputErrorInResult` writes this message
            // and nothing else, with no `Error parsing format string: … at
            // {index}` envelope around it. The formatter name quoted is the
            // one the template writes, not this formatter's own — the two
            // differ when names are matched case-insensitively.
            let issue = std::format!(
                "Formatter named '{}' found no registered template named '{name}'",
                info.placeholder().formatter_name
            );
            return Err(info.plain_error_here(&issue));
        };

        // .NET `FormatAsChild(template, CurrentValue)`: the template is
        // rendered with the current value pushed as its scope, into the same
        // output and with the placeholder's alignment.
        let value = info.current();
        info.format_as_child(template, value)?;
        Ok(true)
    }
}

/// .NET `Format.RawText` for a format that holds no placeholder: the literal
/// text with escape sequences resolved.
///
/// A sequence that resolves to nothing is left as written by the parser and
/// reported by `LiteralText::escape_error`, which .NET raises as an
/// `ArgumentException` while `Format.ToString()` concatenates the literals —
/// so the first failing literal decides, and the message carries no envelope.
fn raw_text(info: &FormattingInfo<'_>, format: &Format) -> Result<String, Error> {
    if let Some((message, start)) = format.first_escape_error() {
        return Err(info.plain_error(message, start));
    }
    Ok(format.literal_text())
}

/// Why [`TemplateFormatter::register`] rejected a template.
#[derive(Debug)]
pub enum RegisterError {
    /// A template is already registered under that name. .NET's
    /// `Dictionary.Add` throws an `ArgumentException` carrying this message.
    Duplicate(String),
    /// The template itself does not parse. .NET's `ParseFormat` throws
    /// whatever the parser's `ParseErrorAction` says.
    Parse(Error),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::Duplicate(name) => write!(
                f,
                "An item with the same key has already been added. Key: {name}"
            ),
            RegisterError::Parse(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for RegisterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RegisterError::Duplicate(_) => None,
            RegisterError::Parse(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Ported from SmartFormat.NET
    //! `src/SmartFormat.Tests/Extensions/TemplateFormatterTests.cs`, plus the
    //! cases probed against the pinned SmartFormat.NET 3.6.1 package.
    //!
    //! `TemplateFormatter` is not part of .NET's `CreateDefaultSmartFormat`,
    //! so every test registers it by hand, as the .NET tests do with
    //! `AddExtensions`. The `Templates_can_be_reused` cases need the list
    //! formatter, which lands on its own branch, so only their template half
    //! is ported here.
    //!
    //! An error is asserted by its message. The one error this formatter
    //! raises itself is a plain .NET `FormatException` rather than a
    //! `FormattingException`, so the message is the bare issue with no
    //! `Error parsing format string: … at {index}` envelope.

    use std::collections::BTreeMap;

    use super::*;
    use crate::extensions::ConditionalFormatter;
    use crate::formatter::{DefaultFormatter, FormatterRegistry};
    use crate::parsing::ParserSettings;
    use crate::settings::{ErrorAction, SmartSettings};
    use crate::value::Value;
    use crate::SmartFormatter;

    /// The templates the .NET fixture registers. `LAST` collides with `last`
    /// when names are matched case-insensitively, which .NET's fixture — and
    /// [`TemplateFormatter::register`] — reject, so it is registered only in
    /// the case-sensitive fixture.
    fn templates_of(
        smart: &SmartFormatter,
        case_sensitivity: CaseSensitivity,
    ) -> TemplateFormatter {
        let mut templates = TemplateFormatter::new(case_sensitivity);
        let mut register = |name: &str, template: &str| {
            templates
                .register(smart.parser(), name, template)
                .unwrap_or_else(|error| panic!("registering {name:?} failed: {error}"));
        };
        register("firstLast", "{First} {Last}");
        register("lastFirst", "{Last}, {First}");
        register("FIRST", "{First.ToUpper}");
        register("last", "{Last.ToLower}");
        if case_sensitivity == CaseSensitivity::CaseSensitive {
            register("LAST", "{Last.ToUpper}");
        }
        register("NESTED", "{:t:FIRST} {:t:last}");
        templates
    }

    /// A formatter with the .NET fixture's templates registered.
    ///
    /// Only the formatter under test and the always-last `DefaultFormatter`,
    /// so these tests pin this formatter rather than the order of the default
    /// registry. The .NET default of `FormatErrorAction.ThrowError` — our
    /// [`ErrorAction::Error`] — so a formatting error surfaces in the tests.
    fn smart_with(case_sensitivity: CaseSensitivity) -> SmartFormatter {
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            case_sensitive: case_sensitivity,
            ..SmartSettings::default()
        });
        let templates = templates_of(&smart, case_sensitivity);
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(templates));
        formatters.push(Box::new(DefaultFormatter));
        smart
    }

    fn insensitive() -> SmartFormatter {
        smart_with(CaseSensitivity::CaseInsensitive)
    }

    fn sensitive() -> SmartFormatter {
        smart_with(CaseSensitivity::CaseSensitive)
    }

    fn person() -> Value {
        map([
            ("First", Value::from("Scott")),
            ("Last", Value::from("Rippey")),
        ])
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
        smart
            .format(template, &person())
            .unwrap_or_else(|error| panic!("{template:?} failed: {error}"))
    }

    fn error_of(smart: &SmartFormatter, template: &str) -> String {
        match smart.format(template, &person()) {
            Err(Error::Format { message, .. }) => message,
            other => panic!("{template:?}: expected a formatting error, got {other:?}"),
        }
    }

    #[test]
    fn name_and_defaults_match_dotnet() {
        let formatter = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        assert_eq!(formatter.name(), "t");
        // .NET's `CanAutoDetect` setter throws for `true`, so there is nothing
        // to turn on here.
        assert!(!formatter.can_auto_detect());
        assert_eq!(formatter.case_sensitivity(), CaseSensitivity::CaseSensitive);
        assert!(formatter.is_empty());
        assert_eq!(
            TemplateFormatter::new(CaseSensitivity::CaseSensitive)
                .with_name("template")
                .name(),
            "template"
        );
    }

    #[test]
    fn only_the_name_selects_the_formatter() {
        // The obsolete .NET `Names` array holds "template" as well, but only
        // `Name` is looked up (probed against 3.6.1).
        let smart = sensitive();
        assert_eq!(format(&smart, "{:t:firstLast}"), "Scott Rippey");
        assert!(error_of(&smart, "{:template:firstLast}")
            .starts_with("Error parsing format string: No suitable Formatter could be found"),);
    }

    #[test]
    fn sanity_test() {
        // Without the template formatter in play the placeholders render
        // themselves.
        assert_eq!(format(&insensitive(), "{First} {Last}"), "Scott Rippey");
    }

    #[test]
    fn template_can_be_called_with_options_or_with_format_string() {
        let smart = insensitive();
        for template in [
            "{:t(firstLast)}",
            "{:t:firstLast}",
            "{:t():firstLast}",
            // Options win: the format is never even looked at.
            "{:t(firstLast):IGNORED}",
        ] {
            assert_eq!(format(&smart, template), "Scott Rippey", "{template}");
        }
    }

    #[test]
    fn simple_templates_work_as_expected() {
        let smart = sensitive();
        for (template, expected) in [
            ("{:t:lastFirst}", "Rippey, Scott"),
            ("{:t:FIRST}", "SCOTT"),
            ("{:t:last}", "rippey"),
            ("{:t:LAST}", "RIPPEY"),
        ] {
            assert_eq!(format(&smart, template), expected, "{template}");
        }
    }

    #[test]
    fn multiple_templates_can_be_used() {
        let smart = insensitive();
        for (template, expected) in [
            ("{:t:FIRST} {:t:last}", "SCOTT rippey"),
            (
                "{:t:firstLast} | {:t:lastFirst}",
                "Scott Rippey | Rippey, Scott",
            ),
        ] {
            assert_eq!(format(&smart, template), expected, "{template}");
        }
    }

    #[test]
    fn templates_can_be_nested() {
        // A template that renders other templates, picked by a condition on
        // the second argument.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        });
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        for (name, template) in [
            ("salutation", "{1:cond:{:t:sal_formal}|{:t:sal_informal}}"),
            ("sal_formal", "Dear Mr {LastName}"),
            ("sal_informal", "Hi {Nickname}"),
        ] {
            templates.register(smart.parser(), name, template).unwrap();
        }
        let formatters = smart.formatters_mut();
        *formatters = FormatterRegistry::empty();
        formatters.push(Box::new(templates));
        formatters.push(Box::new(ConditionalFormatter::new()));
        formatters.push(Box::new(DefaultFormatter));

        let person = map([
            ("FirstName", Value::from("Joseph")),
            ("Nickname", Value::from("Joe")),
            ("LastName", Value::from("Doe")),
        ]);
        for (formal, expected) in [(true, "Dear Mr Doe:"), (false, "Hi Joe:")] {
            let args = Value::List(vec![person.clone(), Value::Bool(formal)]);
            assert_eq!(smart.format("{0:t(salutation)}:", &args).unwrap(), expected);
        }
    }

    #[test]
    fn templates_are_case_sensitive() {
        let smart = sensitive();
        for (template, name) in [
            ("{:t:first}", "first"),
            ("{:t:firstlast}", "firstlast"),
            ("{:t:LaSt}", "LaSt"),
        ] {
            assert_eq!(
                error_of(&smart, template),
                not_registered("t", name),
                "{template}"
            );
        }
    }

    #[test]
    fn templates_can_be_case_insensitive() {
        let smart = insensitive();
        for (template, expected) in [
            ("{:t:first}", "SCOTT"),
            ("{:t:FIRST}", "SCOTT"),
            ("{:t:last}", "rippey"),
            // `LAST` is not registered in the case-insensitive fixture, so
            // this reaches `last`.
            ("{:t:LAST}", "rippey"),
            ("{:t:nested}", "SCOTT rippey"),
            ("{:t:NESTED}", "SCOTT rippey"),
            ("{:t:NeStEd}", "SCOTT rippey"),
            ("{:t:fIrStLaSt}", "Scott Rippey"),
        ] {
            assert_eq!(format(&smart, template), expected, "{template}");
        }
        // The formatter's own name is matched by the settings too, and the
        // error quotes it as the template writes it.
        assert_eq!(error_of(&smart, "{:T:nope}"), not_registered("T", "nope"));
    }

    #[test]
    fn an_unregistered_template_is_an_error() {
        let smart = sensitive();
        for (template, name) in [
            ("{:t:does-not-exist}", "does-not-exist"),
            ("{:t(nope)}", "nope"),
            // No options and an empty format: the empty name, which nothing is
            // registered under.
            ("{:t:}", ""),
            ("{:t()}", ""),
            ("{:t():}", ""),
        ] {
            assert_eq!(
                error_of(&smart, template),
                not_registered("t", name),
                "{template}"
            );
        }
    }

    #[test]
    fn the_empty_name_can_be_registered() {
        let mut smart = SmartFormatter::new(SmartSettings::default());
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates.register(smart.parser(), "", "EMPTY").unwrap();
        smart.formatters_mut().push(Box::new(templates));

        for template in ["{:t:}", "{:t()}", "{:t():}"] {
            assert_eq!(format(&smart, template), "EMPTY", "{template}");
        }
    }

    #[test]
    fn a_format_holding_a_placeholder_is_declined() {
        // .NET returns `false` rather than looking for a template named
        // `{First}`, and no other formatter takes the placeholder either.
        let smart = sensitive();
        for template in ["{:t:{First}}", "{:t:x{First}}"] {
            assert!(
                error_of(&smart, template).starts_with(
                    "Error parsing format string: No suitable Formatter could be found"
                ),
                "{template}"
            );
        }
        // With options there is a name, so the format is ignored, placeholder
        // and all.
        assert_eq!(format(&smart, "{:t(firstLast):{Nope}}"), "Scott Rippey");
    }

    #[test]
    fn the_template_name_resolves_escape_sequences() {
        // .NET takes the name from `Format.RawText`, which is the literal text
        // with escape sequences resolved — probed against 3.6.1.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::Error,
            ..SmartSettings::default()
        });
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates
            .register(smart.parser(), r"back\slash", "BS")
            .unwrap();
        templates
            .register(smart.parser(), "{brace}", "BRACE")
            .unwrap();
        smart.formatters_mut().push(Box::new(templates));

        assert_eq!(format(&smart, r"{:t:back\\slash}"), "BS");
        assert_eq!(format(&smart, r"{:t(back\\slash)}"), "BS");
        assert_eq!(format(&smart, r"{:t:\{brace\}}"), "BRACE");

        // A sequence that resolves to nothing fails the way writing the
        // literal would, with .NET's bare `ArgumentException` message.
        assert_eq!(
            error_of(&smart, r"{:t:back\slash}"),
            r#"Unrecognized escape sequence "\s" in literal."#
        );
        // The parser reads past the end of a short `\u` sequence, leaving a
        // literal that reports the rest of the placeholder.
        assert_eq!(
            error_of(&smart, r"{:t:a\u12}"),
            r#"Unrecognized escape sequence in literal: "\u12}""#
        );
    }

    #[test]
    fn unresolvable_options_fail_the_call() {
        // .NET resolves escape sequences in the `FormatterOptions` getter,
        // outside the evaluator's error handling, so this throws whatever the
        // error action is (probed).
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::OutputErrorInResult,
            ..SmartSettings::default()
        });
        smart.formatters_mut().push(Box::new(TemplateFormatter::new(
            CaseSensitivity::CaseSensitive,
        )));

        match smart.format(r"{:t(a\qb)}", &person()) {
            Err(Error::Escape { message, .. }) => {
                assert_eq!(message, r#"Unrecognized escape sequence "\q" in literal."#);
            }
            other => panic!("expected an escape error, got {other:?}"),
        }
    }

    #[test]
    fn the_alignment_reaches_the_template() {
        // .NET's `FormatAsChild` passes the placeholder's alignment down, so
        // it pads the template's literals rather than the whole result: the
        // single space of "{First} {Last}" becomes 20 columns wide, whichever
        // way it is aligned (probed).
        let smart = sensitive();
        for template in ["[{,20:t(firstLast)}]", "[{,-20:t(firstLast)}]"] {
            assert_eq!(
                format(&smart, template),
                std::format!("[Scott{}Rippey]", " ".repeat(20)),
                "{template}"
            );
        }
    }

    #[test]
    fn a_template_is_parsed_once_with_the_given_parser() {
        // .NET parses with `_formatter.Parser`, the parser of the formatter
        // the templates belong to — settings and all.
        let mut smart = SmartFormatter::with_parser_settings(
            SmartSettings::default(),
            ParserSettings {
                convert_character_string_literals: false,
                ..ParserSettings::default()
            },
        );
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates.register(smart.parser(), "tab", r"a\tb").unwrap();
        // The parsed template is kept, not the string it came from.
        assert_eq!(templates.len(), 1);
        assert_eq!(templates.get("tab").unwrap().raw(), r"a\tb");
        smart.formatters_mut().push(Box::new(templates));

        assert_eq!(format(&smart, "{:t:tab}"), r"a\tb");
    }

    #[test]
    fn register_rejects_a_duplicate_name() {
        // .NET's `Dictionary.Add` throws rather than overwriting.
        let smart = SmartFormatter::new(SmartSettings::default());
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates.register(smart.parser(), "dup", "one").unwrap();
        let error = templates
            .register(smart.parser(), "dup", "two")
            .unwrap_err();
        assert!(matches!(&error, RegisterError::Duplicate(name) if name == "dup"));
        assert_eq!(
            error.to_string(),
            "An item with the same key has already been added. Key: dup"
        );
        // The first registration stands.
        assert_eq!(templates.get("dup").unwrap().raw(), "one");

        // A name that differs only in case collides when names are matched
        // case-insensitively, as .NET's comparer does.
        let mut insensitive = TemplateFormatter::new(CaseSensitivity::CaseInsensitive);
        insensitive.register(smart.parser(), "dup", "one").unwrap();
        let error = insensitive
            .register(smart.parser(), "DUP", "two")
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "An item with the same key has already been added. Key: DUP"
        );
        // Case-sensitive matching keeps the two apart.
        let mut sensitive = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        sensitive.register(smart.parser(), "dup", "one").unwrap();
        sensitive.register(smart.parser(), "DUP", "two").unwrap();
        assert_eq!(sensitive.len(), 2);
    }

    #[test]
    fn register_reports_a_template_that_does_not_parse() {
        let smart = SmartFormatter::new(SmartSettings::default());
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        let error = templates
            .register(smart.parser(), "bad", "{unclosed")
            .unwrap_err();
        assert!(
            matches!(error, RegisterError::Parse(Error::Parse { .. })),
            "{error:?}"
        );
        assert!(templates.is_empty());
    }

    #[test]
    fn a_duplicate_name_that_does_not_parse_reports_the_parse_error() {
        // .NET's `Register` is `var parsed = Parser.ParseFormat(template);
        // _templates.Add(name, parsed);` — the parse runs before the
        // dictionary is touched, so the parse error wins over the duplicate.
        // Probed against 3.6.1: registering "dup" twice, the second time with
        // "{unclosed", throws `ParsingErrors`, not the `ArgumentException`
        // `Dictionary.Add` raises for a duplicate key.
        let smart = SmartFormatter::new(SmartSettings::default());
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates.register(smart.parser(), "dup", "one").unwrap();
        let error = templates
            .register(smart.parser(), "dup", "{unclosed")
            .unwrap_err();
        assert!(
            matches!(error, RegisterError::Parse(Error::Parse { .. })),
            "{error:?}"
        );
        // Neither check changed the registry.
        assert_eq!(templates.len(), 1);
        assert_eq!(templates.get("dup").unwrap().raw(), "one");
    }

    #[test]
    fn remove_and_clear() {
        let smart = SmartFormatter::new(SmartSettings::default());
        let mut templates = templates_of(&smart, CaseSensitivity::CaseSensitive);
        let registered = templates.len();
        assert!(templates.remove("firstLast"));
        assert!(!templates.remove("firstLast"));
        assert!(!templates.remove("does-not-exist"));
        assert_eq!(templates.len(), registered - 1);
        // A removed name can be registered again.
        templates
            .register(smart.parser(), "firstLast", "AGAIN")
            .unwrap();
        assert_eq!(templates.get("firstLast").unwrap().raw(), "AGAIN");

        templates.clear();
        assert!(templates.is_empty());
        assert!(templates.get("firstLast").is_none());

        // `Remove` uses the same comparer as the lookup.
        let mut insensitive = TemplateFormatter::new(CaseSensitivity::CaseInsensitive);
        insensitive.register(smart.parser(), "a", "A").unwrap();
        assert!(insensitive.remove("A"));
        assert!(insensitive.is_empty());
    }

    #[test]
    fn compatibility_mode_never_reaches_the_formatter() {
        // `StringFormatCompatibility` runs `DefaultFormatter` alone, and the
        // parser does not even read formatter names.
        let mut smart = SmartFormatter::new(SmartSettings {
            string_format_compatibility: true,
            ..SmartSettings::default()
        });
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates.register(smart.parser(), "x", "X").unwrap();
        smart.formatters_mut().push(Box::new(templates));

        // A string ignores the specifier `t:x`, so the argument comes out as
        // it is rather than as the template (probed).
        let args = Value::List(vec![Value::from("hello")]);
        assert_eq!(smart.format("{0:t:x}", &args).unwrap(), "hello");
    }

    #[test]
    fn an_error_inside_a_template_is_reported_against_the_outer_template() {
        // A divergence, and not this formatter's to fix: .NET builds the
        // `FormattingException` from the failing item's own `BaseString`, so
        // an error inside a registered template quotes *the template* —
        // `…selector named "Nope" at 1\n{Nope}\n-^`. `Engine` quotes the one
        // string it is rendering, so the caret line here belongs to the outer
        // template while the index is an offset into the registered one. The
        // issue itself is the same, which is all this pins.
        let mut smart = SmartFormatter::new(SmartSettings {
            format_error_action: ErrorAction::OutputErrorInResult,
            ..SmartSettings::default()
        });
        let mut templates = TemplateFormatter::new(CaseSensitivity::CaseSensitive);
        templates.register(smart.parser(), "bad", "{Nope}").unwrap();
        smart.formatters_mut().push(Box::new(templates));

        let rendered = format(&smart, "x{:t:bad}y");
        assert!(
            rendered.contains(
                r#"Error parsing format string: No source extension could handle the selector named "Nope" at 1"#
            ),
            "{rendered}"
        );
        assert!(
            rendered.starts_with('x') && rendered.ends_with('y'),
            "{rendered}"
        );
    }

    /// The message .NET's `FormatException` carries when no template is
    /// registered under the name a placeholder asks for.
    fn not_registered(formatter: &str, name: &str) -> String {
        std::format!("Formatter named '{formatter}' found no registered template named '{name}'")
    }
}
