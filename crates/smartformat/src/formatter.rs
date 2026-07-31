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

use crate::error::Error;
use crate::fmt::culture::{self, CultureData};
#[cfg(feature = "time")]
use crate::fmt::date;
use crate::fmt::number::{self, Number};
use crate::fmt::FormatSpecError;
use crate::parsing::chars::ALIGNMENT_OPERATOR;
use crate::parsing::{Format, FormatItem, Parser, ParserSettings, Placeholder, Selector};
use crate::settings::{CaseSensitivity, ErrorAction, SmartSettings};
use crate::sources::{SelectorInfo, SourceRegistry};
use crate::value::Value;

/// The value a nameless placeholder resolves to when there is nothing in scope.
static NULL: Value = Value::Null;

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

    /// The M1 formatters: [`DefaultFormatter`] only.
    pub fn new() -> Self {
        Self {
            formatters: vec![Box::new(DefaultFormatter)],
        }
    }

    /// Adds a formatter, which is consulted before the always-last
    /// [`DefaultFormatter`] if one is registered.
    pub fn insert(&mut self, index: usize, formatter: Box<dyn Formatter>) {
        self.formatters.insert(index, formatter);
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
            base: &format.raw,
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

// ---------------------------------------------------------------------------
// The evaluator
// ---------------------------------------------------------------------------

/// One format call, ported from .NET `Evaluator` plus `FormatDetails`.
struct Engine<'a> {
    smart: &'a SmartFormatter,
    args: &'a [Value],
    culture: &'a CultureData,
    /// The whole template, quoted by error messages (.NET
    /// `FormatItem.BaseString`).
    base: &'a str,
}

impl<'e> Engine<'e> {
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
            Some(prefix) => prefix.encode_utf16().count(),
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

        // Compatibility mode bypasses every extension but DefaultFormatter,
        // including the auto-detecting ones.
        if self.smart.settings.string_format_compatibility {
            let handled = match self.smart.formatters.default_formatter() {
                Some(formatter) => formatter.try_evaluate_format(&mut info)?,
                None => false,
            };
            return if handled {
                Ok(())
            } else {
                Err(self.no_formatter_error(placeholder))
            };
        }

        let name = &placeholder.formatter_name;
        if !name.is_empty() {
            // .NET reports a missing formatter and a formatter that declined
            // the value the same way.
            let handled = match self
                .smart
                .formatters
                .find(name, self.smart.settings.case_sensitive)
            {
                Some(formatter) => formatter.try_evaluate_format(&mut info)?,
                None => false,
            };
            return if handled {
                Ok(())
            } else {
                Err(self.no_formatter_error(placeholder))
            };
        }

        for formatter in &self.smart.formatters.formatters {
            if !formatter.can_auto_detect() {
                continue;
            }
            if formatter.try_evaluate_format(&mut info)? {
                return Ok(());
            }
        }

        Err(self.no_formatter_error(placeholder))
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
            selector.start,
        )
    }

    /// .NET `Evaluator.InvokeFormatters`, which reports a missing formatter,
    /// a formatter that declined the value, and no auto-detecting formatter
    /// at all with one message — and positions it at the *ordinal* index of
    /// the last evaluated selector, not at an offset into the template.
    fn no_formatter_error(&self, placeholder: &Placeholder) -> Error {
        let index = placeholder
            .selectors
            .iter()
            .rfind(|selector| !skip_selector(selector))
            .map_or(0, |selector| selector.index);
        self.formatting_error("No suitable Formatter could be found", index)
    }

    /// A .NET `FormattingException`, whose `Message` quotes the template and
    /// points at `index` (`FormattingException.Message`).
    fn formatting_error(&self, issue: &str, index: usize) -> Error {
        Error::Format {
            message: format!(
                "Error parsing format string: {issue} at {index}\n{}\n{}^",
                self.base,
                "-".repeat(index)
            ),
            position: index,
        }
    }
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
        | Error::Escape { message, .. } => message.clone(),
        parse_error => parse_error.to_string(),
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
    let width = text.encode_utf16().count();
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

    pub fn culture(&self) -> &'a CultureData {
        self.engine.culture
    }

    pub fn settings(&self) -> &'a SmartSettings {
        &self.engine.smart.settings
    }

    /// Writes text to the output, applying the placeholder's alignment.
    pub fn write(&mut self, text: &str) {
        let fill = self.engine.smart.settings.alignment_fill_character;
        write_aligned(self.output, text, self.alignment, fill);
    }

    /// Renders `format` with `value` as the current scope, appending to the
    /// same output (.NET `IFormattingInfo.FormatAsChild`).
    pub fn format_as_child(&mut self, format: &Format, value: &Value) -> Result<(), Error> {
        let mut scopes = self.scopes.to_vec();
        scopes.push(value);
        self.engine
            .write_format(format, &scopes, self.alignment, self.output)
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
        let position = error_position(info.placeholder());
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
            // A deliberate divergence: .NET falls back to `object.ToString()`
            // and renders the CLR type name (`System.Object[]`,
            // `System.Collections.Generic.Dictionary`2[...]`), which is never
            // what a template author wanted. We fail loudly instead. See
            // DESIGN.md, "Known divergences".
            Value::List(_) => {
                return Err(Error::Format {
                    message: "Default formatting of a list is not supported; use a formatter such as \"list\"".to_owned(),
                    position,
                })
            }
            Value::Map(_) => {
                return Err(Error::Format {
                    message: "Default formatting of a map is not supported; select a value from it"
                        .to_owned(),
                    position,
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
fn spec_result(
    result: Result<String, FormatSpecError>,
    position: usize,
    invalid_message: &str,
) -> Result<String, Error> {
    result.map_err(|error| match error {
        FormatSpecError::Unsupported(spec) => Error::UnsupportedSpec {
            message: format!("unsupported format spec: {spec}"),
            spec,
            position,
        },
        FormatSpecError::Invalid(_) => Error::Format {
            message: invalid_message.to_owned(),
            position,
        },
    })
}
