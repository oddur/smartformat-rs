//! Ported from SmartFormat.NET `src/SmartFormat/Core/Parsing/Parser.cs`.
//!
//! The .NET parser walks the input once, keeping a bag of indices and mutating a
//! tree of pooled objects through parent pointers. This port keeps the same
//! single pass and the same index bookkeeping, but builds the tree on a stack of
//! unfinished placeholders and materializes owned strings as it goes.

use std::collections::HashSet;

use super::chars::{
    ALIGNMENT_OPERATOR, CHAR_LITERAL_ESCAPE_CHAR, FORMATTER_NAME_SEPARATOR,
    FORMATTER_OPTIONS_BEGIN_CHAR, FORMATTER_OPTIONS_END_CHAR, FORMAT_OPTIONS_TERMINATOR_CHARS,
    LIST_INDEX_END_CHAR, NULLABLE_OPERATOR, PLACEHOLDER_BEGIN_CHAR, PLACEHOLDER_END_CHAR,
};
use super::escaped_literal::{self, try_get_char, unescape};
use super::settings::{CharSet, ParserSettings};
use super::{Format, FormatItem, LiteralText, Placeholder, Selector};
use crate::error::{Error, ParseError};
use crate::settings::ErrorAction;

pub(crate) const TOO_MANY_CLOSING_BRACES: &str = "Format string has too many closing braces";
pub(crate) const TRAILING_OPERATORS_IN_SELECTOR: &str =
    "There are illegal trailing operators in the selector";
pub(crate) const INVALID_CHARACTERS_IN_SELECTOR: &str = "Invalid character in the selector";
pub(crate) const MISSING_CLOSING_BRACE: &str = "Format string is missing a closing brace";
const UNRECOGNIZED_ESCAPE_AT_END: &str = "Unrecognized escape sequence at the end of the literal";

/// Parses format strings into a [`Format`] tree.
///
/// Settings are read once, when the parser is created.
#[derive(Debug)]
pub struct Parser {
    settings: ParserSettings,
    selector_chars: CharSet,
    operator_chars: HashSet<char>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new(ParserSettings::default())
    }
}

impl Parser {
    /// Creates a parser for the given settings.
    pub fn new(settings: ParserSettings) -> Self {
        let selector_chars = settings.selector_chars();
        let operator_chars = settings.operator_chars();
        Self {
            settings,
            selector_chars,
            operator_chars,
        }
    }

    /// The settings this parser was created with.
    pub fn settings(&self) -> &ParserSettings {
        &self.settings
    }

    /// Parses a format string.
    ///
    /// Syntax errors are collected and then handled according to
    /// [`ParserSettings::error_action`]: they are returned as
    /// [`Error::Parse`] only for [`ErrorAction::Error`]; the other actions
    /// recover and return a [`Format`].
    pub fn parse(&self, input: &str) -> Result<Format, Error> {
        State::new(self, input).run()
    }
}

/// Where the main loop currently is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    /// Top-level literal text, or literal text inside a placeholder's format.
    LiteralText,
    /// The selectors and the formatter name of a placeholder.
    SelectorHeader,
}

/// A syntax error, positioned in characters (as in .NET) rather than bytes.
struct Issue {
    message: String,
    index: usize,
    length: usize,
}

struct State<'a> {
    parser: &'a Parser,
    input: &'a str,
    chars: Vec<char>,
    /// Byte offset of every character, plus the length of the input.
    offsets: Vec<usize>,
    len: usize,

    current: usize,
    last_end: usize,
    operator: usize,
    selector: usize,
    named_formatter_start: Option<usize>,
    named_formatter_options_start: Option<usize>,
    named_formatter_options_end: Option<usize>,

    /// The format items are currently added to.
    result: Format,
    /// Placeholders whose format is still being parsed, with the format they belong to.
    stack: Vec<(Placeholder, Format)>,
    current_placeholder: Option<Placeholder>,
    nested_depth: usize,
    issues: Vec<Issue>,
}

impl<'a> State<'a> {
    fn new(parser: &'a Parser, input: &'a str) -> Self {
        let chars: Vec<char> = input.chars().collect();
        let mut offsets: Vec<usize> = input.char_indices().map(|(index, _)| index).collect();
        offsets.push(input.len());
        let len = chars.len();

        Self {
            parser,
            input,
            chars,
            offsets,
            len,
            current: 0,
            last_end: 0,
            operator: 0,
            selector: 0,
            named_formatter_start: None,
            named_formatter_options_start: None,
            named_formatter_options_end: None,
            result: Format {
                items: Vec::new(),
                raw: String::new(),
                start: 0,
                end: input.len(),
            },
            stack: Vec::new(),
            current_placeholder: None,
            nested_depth: 0,
            issues: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Format, Error> {
        let mut context = Context::LiteralText;

        while self.current < self.len {
            let input_char = self.chars[self.current];
            match context {
                Context::SelectorHeader => self.process_selector(input_char, &mut context),
                Context::LiteralText => self.process_literal_text(input_char, &mut context)?,
            }
            self.current += 1;
        }

        self.finalize()?;
        set_raw(&mut self.result, self.input);

        if self.issues.is_empty() {
            return Ok(self.result);
        }
        self.handle_errors()
    }

    // ----- index helpers -------------------------------------------------

    /// Adds to a character index, never going past the end of the input.
    fn safe_add(&self, index: usize, add: usize) -> usize {
        (index + add).min(self.len)
    }

    /// The byte offset of a character index.
    fn byte(&self, index: usize) -> usize {
        self.offsets[index.min(self.len)]
    }

    fn text(&self, start: usize, end: usize) -> String {
        let start = start.min(self.len);
        let end = end.clamp(start, self.len);
        self.chars[start..end].iter().collect()
    }

    fn add_issue(&mut self, message: String, start: usize, end: usize) {
        self.issues.push(Issue {
            message,
            index: start,
            length: end.saturating_sub(start),
        });
    }

    fn fatal(&self, message: impl Into<String>, index: usize) -> Error {
        Error::Parse {
            errors: vec![ParseError {
                message: message.into(),
                position: self.byte(index),
            }],
        }
    }

    // ----- literal text --------------------------------------------------

    /// Handles a character of literal text: the start of a placeholder, the end
    /// of a nested placeholder's format, an escape sequence, or a formatter name.
    fn process_literal_text(
        &mut self,
        input_char: char,
        context: &mut Context,
    ) -> Result<(), Error> {
        if input_char == PLACEHOLDER_BEGIN_CHAR {
            self.add_literal_chars_parsed_before()?;
            if self.escape_like_string_format(PLACEHOLDER_BEGIN_CHAR) {
                return Ok(());
            }
            self.create_new_placeholder();
            *context = Context::SelectorHeader;
        } else if input_char == PLACEHOLDER_END_CHAR {
            self.add_literal_chars_parsed_before()?;
            if self.escape_like_string_format(PLACEHOLDER_END_CHAR) {
                return Ok(());
            }
            if self.has_processed_too_many_closing_braces() {
                return Ok(());
            }
            self.finish_placeholder_format();
        } else if input_char == CHAR_LITERAL_ESCAPE_CHAR
            && (self.parser.settings.convert_character_string_literals
                || !self.parser.settings.string_format_compatibility)
        {
            self.parse_alternative_escaping()?;
        } else if self.named_formatter_start.is_some() {
            self.parse_named_formatter()?;
        }

        Ok(())
    }

    /// Builds a literal item, resolving an escape sequence if the text is one.
    /// The parser gives every escape sequence a literal item of its own.
    fn make_literal(&self, start: usize, end: usize) -> Result<LiteralText, Error> {
        let start = start.min(self.len);
        let end = end.clamp(start, self.len);
        let span = &self.chars[start..end];
        let raw: String = span.iter().collect();
        let convert = self.parser.settings.convert_character_string_literals;

        let text = if span.is_empty() {
            String::new()
        } else if convert && span[0] == CHAR_LITERAL_ESCAPE_CHAR {
            unescape(span, false, true).map_err(|message| self.fatal(message, start))?
        } else if !convert
            && span.len() == 2
            && span[0] == span[1]
            && span[0] == CHAR_LITERAL_ESCAPE_CHAR
        {
            // Special case: the escape character escaping itself.
            CHAR_LITERAL_ESCAPE_CHAR.to_string()
        } else {
            raw.clone()
        };

        Ok(LiteralText {
            text,
            raw,
            start: self.byte(start),
            end: self.byte(end),
        })
    }

    fn push_literal(&mut self, start: usize, end: usize) -> Result<(), Error> {
        let literal = self.make_literal(start, end)?;
        self.result.items.push(FormatItem::Literal(literal));
        Ok(())
    }

    /// Closes the literal text that ends at the current character.
    fn add_literal_chars_parsed_before(&mut self) -> Result<(), Error> {
        if self.current != self.last_end {
            self.push_literal(self.last_end, self.current)?;
        }
        self.last_end = self.safe_add(self.current, 1);
        Ok(())
    }

    /// With `string.Format` compatibility, `{{` and `}}` are escaped braces.
    fn escape_like_string_format(&mut self, brace: char) -> bool {
        if !self.parser.settings.string_format_compatibility {
            return false;
        }

        if self.last_end < self.len && self.chars[self.last_end] == brace {
            self.current = self.safe_add(self.current, 1);
            return true;
        }

        false
    }

    /// A closing brace with nothing to close stays in the output as literal text.
    fn has_processed_too_many_closing_braces(&mut self) -> bool {
        if !self.stack.is_empty() {
            return false;
        }

        let brace = PLACEHOLDER_END_CHAR.to_string();
        self.result.items.push(FormatItem::Literal(LiteralText {
            text: brace.clone(),
            raw: brace,
            start: self.byte(self.current),
            end: self.byte(self.current + 1),
        }));
        self.add_issue(
            TOO_MANY_CLOSING_BRACES.to_owned(),
            self.current,
            self.current + 1,
        );

        true
    }

    /// Handles `\{`, `\}` and character literals such as `\n` or `•`.
    fn parse_alternative_escaping(&mut self) -> Result<(), Error> {
        let index_next_char = self.current + 1;
        if index_next_char >= self.len {
            return Err(self.fatal(UNRECOGNIZED_ESCAPE_AT_END, self.current));
        }

        if self.chars[index_next_char] == PLACEHOLDER_BEGIN_CHAR
            || self.chars[index_next_char] == PLACEHOLDER_END_CHAR
        {
            // The brace itself starts the next run of literal text.
            if self.current != self.last_end {
                self.push_literal(self.last_end, self.current)?;
            }
            self.last_end = self.safe_add(self.current, 1);
            self.current += 1;
        } else {
            if self.current != self.last_end {
                self.push_literal(self.last_end, self.current)?;
            }

            self.last_end = if self.chars[index_next_char] == 'u' {
                // The escape character, the 'u' and 4 hex digits — twice when
                // the sequence is the high half of a surrogate pair, so both
                // halves land in one literal and can be joined.
                self.safe_add(self.current, self.unicode_escape_len())
            } else {
                self.safe_add(self.current, 2)
            };

            self.push_literal(self.current, self.last_end)?;
            // Resume at the end of the sequence: a surrogate pair contains a
            // second escape character, which must not start another sequence.
            self.current = self.last_end - 1;
        }

        Ok(())
    }

    /// How many characters the `\uXXXX` sequence at [`current`](Self::current)
    /// spans: 12 for a surrogate pair, 6 otherwise.
    fn unicode_escape_len(&self) -> usize {
        let is_pair = escaped_literal::unicode(&self.chars, self.current + 2)
            .is_ok_and(escaped_literal::is_high_surrogate)
            && self.chars.get(self.current + 6) == Some(&CHAR_LITERAL_ESCAPE_CHAR)
            && self.chars.get(self.current + 7) == Some(&'u');
        if is_pair {
            12
        } else {
            6
        }
    }

    // ----- placeholders --------------------------------------------------

    fn create_new_placeholder(&mut self) {
        self.nested_depth += 1;
        self.current_placeholder = Some(Placeholder {
            // Inherit the alignment of the enclosing placeholder, if any.
            alignment: self
                .stack
                .last()
                .map_or(0, |(placeholder, _)| placeholder.alignment),
            nested_depth: self.nested_depth,
            start: self.byte(self.current),
            end: self.byte(self.len),
            ..Placeholder::default()
        });
        self.operator = self.safe_add(self.current, 1);
        self.selector = 0;
        self.named_formatter_start = None;
    }

    fn finish_placeholder(&mut self, mut placeholder: Placeholder, end: usize) {
        placeholder.end = self.byte(end);
        placeholder.raw = self.input[placeholder.start..placeholder.end].to_owned();
        self.result.items.push(FormatItem::Placeholder(placeholder));
    }

    /// Closes the format of a nested placeholder on `}`.
    fn finish_placeholder_format(&mut self) {
        let Some((placeholder, parent)) = self.stack.pop() else {
            return;
        };

        self.nested_depth = self.nested_depth.saturating_sub(1);
        self.result.end = self.byte(self.current);

        let mut placeholder = placeholder;
        let inner = std::mem::replace(&mut self.result, parent);
        placeholder.format = Some(inner);

        let end = self.safe_add(self.current, 1);
        self.finish_placeholder(placeholder, end);

        self.named_formatter_start = None;
        self.named_formatter_options_start = None;
        self.named_formatter_options_end = None;
    }

    // ----- selectors -----------------------------------------------------

    /// Handles a character of a placeholder's header: a selector, an operator,
    /// the `:` starting the format, or the `}` ending the placeholder.
    fn process_selector(&mut self, input_char: char, context: &mut Context) {
        if self.parser.operator_chars.contains(&input_char) {
            // Close the selector before the operator.
            if self.current != self.last_end {
                let selector = self.make_selector(self.last_end, self.current);
                self.add_selector(selector);
                self.selector += 1;
                self.operator = self.current;
            }
            self.last_end = self.safe_add(self.current, 1);
        } else if input_char == FORMATTER_NAME_SEPARATOR {
            self.add_last_selector();

            // Everything after the ':' is the placeholder's format.
            let placeholder = self
                .current_placeholder
                .take()
                .expect("SelectorHeader context implies a current placeholder");
            let new_format = Format {
                items: Vec::new(),
                raw: String::new(),
                start: self.byte(self.current + 1),
                end: self.byte(self.len),
            };
            let parent = std::mem::replace(&mut self.result, new_format);
            self.stack.push((placeholder, parent));

            self.named_formatter_start = if self.parser.settings.string_format_compatibility {
                None
            } else {
                Some(self.last_end)
            };
            self.named_formatter_options_start = None;
            self.named_formatter_options_end = None;

            *context = Context::LiteralText;
        } else if input_char == PLACEHOLDER_END_CHAR {
            self.add_last_selector();

            // The placeholder ends without a format.
            self.nested_depth = self.nested_depth.saturating_sub(1);
            let placeholder = self
                .current_placeholder
                .take()
                .expect("SelectorHeader context implies a current placeholder");
            let end = self.safe_add(self.current, 1);
            self.finish_placeholder(placeholder, end);

            *context = Context::LiteralText;
        } else if !self.parser.selector_chars.is_allowed(input_char) {
            let end = self.safe_add(self.current, 1);
            self.add_issue(
                format!(
                    "'0x{:X}': {}",
                    input_char as u32, INVALID_CHARACTERS_IN_SELECTOR
                ),
                self.current,
                end,
            );
        }
    }

    fn make_selector(&self, start: usize, end: usize) -> Selector {
        let operator_start = self.operator.min(start);
        Selector {
            text: self.text(start, end),
            operator: self.text(operator_start, start),
            index: self.selector,
            start: self.byte(start),
            end: self.byte(end),
        }
    }

    /// Adds a selector to the current placeholder, picking up the alignment if
    /// the selector is one, as in `{name,-10}`.
    fn add_selector(&mut self, selector: Selector) {
        let placeholder = self
            .current_placeholder
            .as_mut()
            .expect("SelectorHeader context implies a current placeholder");

        if selector.operator.starts_with(ALIGNMENT_OPERATOR) {
            if let Ok(alignment) = selector.text.trim().parse::<i32>() {
                placeholder.alignment = alignment;
            }
        }

        placeholder.selectors.push(selector);
    }

    /// Adds the selector ended by `:` or `}`, or reports a trailing operator.
    fn add_last_selector(&mut self) {
        let ends_list_index = self
            .current_placeholder
            .as_ref()
            .and_then(|placeholder| placeholder.selectors.last())
            .is_some_and(|last| last.end > last.start)
            && self.current == self.operator + 1
            && matches!(
                self.chars.get(self.operator),
                Some(&LIST_INDEX_END_CHAR) | Some(&NULLABLE_OPERATOR)
            );

        if self.current != self.last_end || ends_list_index {
            let selector = self.make_selector(self.last_end, self.current);
            self.add_selector(selector);
        } else if self.operator != self.current {
            let operator = self.operator;
            let message = format!(
                "'0x{:X}': {}",
                self.chars.get(operator).copied().unwrap_or_default() as u32,
                TRAILING_OPERATORS_IN_SELECTOR
            );
            self.add_issue(message, operator, self.current);
        }

        self.last_end = self.safe_add(self.current, 1);
    }

    // ----- formatter name and options ------------------------------------

    /// Handles `name`, `name(options)` and the `:` that ends either of them.
    /// Anything unexpected leaves the placeholder without a formatter name, and
    /// the text stays literal.
    fn parse_named_formatter(&mut self) -> Result<(), Error> {
        let Some(name_start) = self.named_formatter_start else {
            return Ok(());
        };
        let input_char = self.chars[self.current];

        if input_char == FORMATTER_OPTIONS_BEGIN_CHAR {
            if name_start == self.current {
                self.named_formatter_start = None;
                return Ok(());
            }
            // Short-circuits the main loop.
            self.parse_format_options();
            return Ok(());
        }

        if input_char != FORMATTER_OPTIONS_END_CHAR && input_char != FORMATTER_NAME_SEPARATOR {
            return Ok(());
        }

        if input_char == FORMATTER_OPTIONS_END_CHAR {
            let has_opening_parenthesis = self.named_formatter_options_start.is_some();
            let next_char_index = self.safe_add(self.current, 1);
            let next_char_is_valid = next_char_index < self.len
                && (self.chars[next_char_index] == FORMATTER_NAME_SEPARATOR
                    || self.chars[next_char_index] == PLACEHOLDER_END_CHAR);

            if !has_opening_parenthesis || !next_char_is_valid {
                self.named_formatter_start = None;
                return Ok(());
            }

            self.named_formatter_options_end = Some(self.current);

            if self.chars[next_char_index] == FORMATTER_NAME_SEPARATOR {
                self.current += 1;
            }
        }

        let name_is_empty = name_start == self.current;
        let missing_closing_parenthesis = self.named_formatter_options_start.is_some()
            && self.named_formatter_options_end.is_none();
        if name_is_empty || missing_closing_parenthesis {
            self.named_formatter_start = None;
            return Ok(());
        }

        self.last_end = self.safe_add(self.current, 1);

        let (name, options_raw, options) = match self.named_formatter_options_start {
            None => (
                self.text(name_start, self.current),
                String::new(),
                String::new(),
            ),
            Some(options_start) => {
                let options_end = self.named_formatter_options_end.unwrap_or(options_start);
                let start = options_start + 1;
                let end = options_end.max(start);
                let options = unescape(
                    &self.chars[start.min(self.len)..end.min(self.len)],
                    true,
                    self.parser.settings.convert_character_string_literals,
                )
                .map_err(|message| self.fatal(message, start))?;
                (
                    self.text(name_start, options_start),
                    self.text(start, end),
                    options,
                )
            }
        };

        if let Some((placeholder, _)) = self.stack.last_mut() {
            placeholder.formatter_name = name;
            placeholder.formatter_options_raw = options_raw;
            placeholder.formatter_options = options;
        }

        // The format starts after the formatter name: for {0:default:N2} that
        // is the second colon.
        self.result.start = self.byte(self.last_end);
        self.named_formatter_start = None;

        Ok(())
    }

    /// Consumes the formatter options up to the terminating character.
    /// This short-circuits the main loop.
    fn parse_format_options(&mut self) {
        self.named_formatter_options_start = Some(self.current);

        if self.is_terminator(self.safe_add(self.current, 1)) {
            // Empty options: `name()`.
            return;
        }

        loop {
            self.current += 1;
            if self.current >= self.len {
                return;
            }

            let next_char = self.chars.get(self.safe_add(self.current, 1)).copied();
            let escapes_next = self.chars[self.current] == CHAR_LITERAL_ESCAPE_CHAR
                && next_char.is_some_and(|next| {
                    FORMAT_OPTIONS_TERMINATOR_CHARS.contains(&next)
                        || try_get_char(next, true, false).is_some()
                });

            if escapes_next {
                self.current = self.safe_add(self.current, 1);
                if self.is_terminator(self.safe_add(self.current, 1)) {
                    return;
                }
                continue;
            }

            // Stop before a terminating character: the main loop handles it.
            if self.is_terminator(self.current + 1) {
                return;
            }
        }
    }

    fn is_terminator(&self, index: usize) -> bool {
        self.chars
            .get(index)
            .is_some_and(|c| FORMAT_OPTIONS_TERMINATOR_CHARS.contains(c))
    }

    // ----- finishing up --------------------------------------------------

    fn finalize(&mut self) -> Result<(), Error> {
        // 1. Is the last item a placeholder that was never closed?
        if !self.stack.is_empty() || self.current_placeholder.is_some() {
            self.add_issue(MISSING_CLOSING_BRACE.to_owned(), self.len, self.len);
            self.result.end = self.byte(self.len);

            if let Some(placeholder) = self.current_placeholder.take() {
                let end = self.len;
                self.finish_placeholder(placeholder, end);
            }
        } else if self.last_end != self.len {
            // 2. The last item must be literal text.
            self.push_literal(self.last_end, self.len)?;
        }

        // Unwind the formats left open by missing closing braces.
        while let Some((mut placeholder, parent)) = self.stack.pop() {
            let inner = std::mem::replace(&mut self.result, parent);
            placeholder.format = Some(inner);
            let end = self.len;
            self.finish_placeholder(placeholder, end);
            self.result.end = self.byte(self.len);
        }

        Ok(())
    }

    fn parse_errors(&self) -> Vec<ParseError> {
        self.issues
            .iter()
            .map(|issue| ParseError {
                message: issue.message.clone(),
                position: self.byte(issue.index),
            })
            .collect()
    }

    /// The .NET `ParsingErrors.Message`, which points at every issue.
    fn error_message(&self) -> String {
        let count = self.issues.len();
        let plural = if count == 1 { "" } else { "s" };
        let joined: Vec<&str> = self
            .issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect();

        let mut arrows = String::new();
        let mut last_arrow = 0;
        for issue in &self.issues {
            arrows.push_str(&"-".repeat(issue.index.saturating_sub(last_arrow)));
            if issue.length > 0 {
                arrows.push_str(&"^".repeat(issue.length));
                last_arrow = issue.index + issue.length;
            } else {
                arrows.push('^');
                last_arrow = issue.index + 1;
            }
        }

        format!(
            "The format string has {count} issue{plural}:\n{}\nIn: \"{}\"\nAt:  {arrows} ",
            joined.join(", "),
            self.input
        )
    }

    /// Applies [`ParserSettings::error_action`] to the collected issues.
    fn handle_errors(self) -> Result<Format, Error> {
        match self.parser.settings.error_action {
            ErrorAction::Error => Err(Error::Parse {
                errors: self.parse_errors(),
            }),
            ErrorAction::MaintainTokens => {
                // Erroneous placeholders keep their tokens as literal text.
                let positions = self.issue_positions();
                let mut result = self.result;
                replace_erroneous_placeholders(&mut result, &positions, false);
                Ok(result)
            }
            ErrorAction::Ignore => {
                // Erroneous placeholders are dropped.
                let positions = self.issue_positions();
                let mut result = self.result;
                replace_erroneous_placeholders(&mut result, &positions, true);
                Ok(result)
            }
            ErrorAction::OutputErrorInResult => {
                let message = self.error_message();
                let end = message.len();
                Ok(Format {
                    items: vec![FormatItem::Literal(LiteralText {
                        text: message.clone(),
                        raw: message.clone(),
                        start: 0,
                        end,
                    })],
                    raw: message,
                    start: 0,
                    end,
                })
            }
        }
    }

    fn issue_positions(&self) -> Vec<usize> {
        self.issues
            .iter()
            .map(|issue| self.byte(issue.index))
            .collect()
    }
}

/// Fills in [`Format::raw`] for a format and every format nested in it. The
/// .NET types keep a reference to the input string instead and slice it on
/// demand (`FormatItem.AsSpan`).
fn set_raw(format: &mut Format, input: &str) {
    let start = format.start.min(input.len());
    let end = format.end.clamp(start, input.len());
    format.raw = input[start..end].to_owned();
    for item in &mut format.items {
        if let FormatItem::Placeholder(placeholder) = item {
            if let Some(nested) = &mut placeholder.format {
                set_raw(nested, input);
            }
        }
    }
}

/// Replaces every top-level placeholder an issue points into, either with the
/// text it was parsed from or with nothing at all.
fn replace_erroneous_placeholders(format: &mut Format, issue_positions: &[usize], drop: bool) {
    for item in &mut format.items {
        let FormatItem::Placeholder(placeholder) = item else {
            continue;
        };
        let has_issue = issue_positions
            .iter()
            .any(|&position| position >= placeholder.start && position <= placeholder.end);
        if !has_issue {
            continue;
        }

        *item = FormatItem::Literal(if drop {
            LiteralText {
                text: String::new(),
                raw: String::new(),
                start: placeholder.start,
                end: placeholder.start,
            }
        } else {
            LiteralText {
                text: placeholder.raw.clone(),
                raw: placeholder.raw.clone(),
                start: placeholder.start,
                end: placeholder.end,
            }
        });
    }
}
