//! Escape sequences inside literal text and formatter options.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/Core/Parsing/EscapedLiteral.cs`.

use std::cell::OnceCell;

use super::chars::{
    CHAR_LITERAL_ESCAPE_CHAR, FORMATTER_NAME_SEPARATOR, FORMATTER_OPTIONS_BEGIN_CHAR,
    FORMATTER_OPTIONS_END_CHAR, PLACEHOLDER_BEGIN_CHAR, PLACEHOLDER_END_CHAR,
};

/// The input text, indexed by *character* — the unit the parser and .NET's
/// `char` loops count in.
///
/// An all-ASCII input, which nearly every format string is, needs no index at
/// all: a character index is a byte index, the character is the byte, and a
/// UTF-16 offset is the same number again. Only an input that holds a
/// non-ASCII character pays for the two tables, and even then the UTF-16
/// offsets — read on error paths only — are counted on demand rather than
/// tabulated up front.
pub(crate) struct Source<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// Every character of the input; empty when [`ascii`](Self::ascii).
    chars: Vec<char>,
    /// Byte offset of every character, plus the length of the input; empty
    /// when [`ascii`](Self::ascii), where the two are the same number.
    offsets: Vec<usize>,
    /// UTF-16 offset of every character, plus the length of the input. Built
    /// by the first [`utf16`](Self::utf16) call, since only error reporting
    /// asks — and never at all when [`ascii`](Self::ascii).
    utf16_offsets: OnceCell<Vec<usize>>,
    /// The number of characters.
    len: usize,
    ascii: bool,
}

impl<'a> Source<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let bytes = text.as_bytes();
        if bytes.is_ascii() {
            return Self {
                text,
                bytes,
                chars: Vec::new(),
                offsets: Vec::new(),
                utf16_offsets: OnceCell::new(),
                len: bytes.len(),
                ascii: true,
            };
        }

        let mut chars: Vec<char> = Vec::with_capacity(text.len());
        let mut offsets: Vec<usize> = Vec::with_capacity(text.len() + 1);
        for (index, character) in text.char_indices() {
            chars.push(character);
            offsets.push(index);
        }
        offsets.push(text.len());
        let len = chars.len();

        Self {
            text,
            bytes,
            chars,
            offsets,
            utf16_offsets: OnceCell::new(),
            len,
            ascii: false,
        }
    }

    /// The number of characters in the input.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The character at `index`, which must be within the input.
    #[inline]
    pub(crate) fn char_at(&self, index: usize) -> char {
        if self.ascii {
            self.bytes[index] as char
        } else {
            self.chars[index]
        }
    }

    /// The character at `index`, or `None` past the end of the input.
    #[inline]
    pub(crate) fn get(&self, index: usize) -> Option<char> {
        if self.ascii {
            self.bytes.get(index).map(|&byte| byte as char)
        } else {
            self.chars.get(index).copied()
        }
    }

    /// The byte offset of a character index, clamped to the end of the input.
    #[inline]
    pub(crate) fn byte(&self, index: usize) -> usize {
        if self.ascii {
            index.min(self.len)
        } else {
            self.offsets[index.min(self.len)]
        }
    }

    /// The UTF-16 code unit offset of a character index — what .NET, whose
    /// strings are UTF-16, reports as the position of a parsing issue.
    ///
    /// Tabulated on demand: only error reporting asks.
    pub(crate) fn utf16(&self, index: usize) -> usize {
        let index = index.min(self.len);
        if self.ascii {
            return index;
        }
        self.utf16_offsets.get_or_init(|| {
            let mut offsets = Vec::with_capacity(self.len + 1);
            let mut units = 0;
            for character in &self.chars {
                offsets.push(units);
                units += character.len_utf16();
            }
            offsets.push(units);
            offsets
        })[index]
    }

    /// The input between two character indices, which must be ordered and
    /// within the input.
    #[inline]
    pub(crate) fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.text[self.byte(start)..self.byte(end)]
    }

    /// A view of the characters in `start..end`, indexed from zero.
    #[inline]
    pub(crate) fn view(&self, start: usize, end: usize) -> View<'_, 'a> {
        View {
            source: self,
            start,
            end,
        }
    }

    /// A view of the whole input.
    #[inline]
    pub(crate) fn all(&self) -> View<'_, 'a> {
        self.view(0, self.len)
    }
}

/// A range of [`Source`] characters, indexed from zero — what .NET passes
/// around as a `ReadOnlySpan<char>`.
#[derive(Clone, Copy)]
pub(crate) struct View<'s, 'a> {
    source: &'s Source<'a>,
    start: usize,
    end: usize,
}

impl View<'_, '_> {
    #[inline]
    fn len(&self) -> usize {
        self.end - self.start
    }

    #[inline]
    fn get(&self, index: usize) -> Option<char> {
        if index < self.len() {
            self.source.get(self.start + index)
        } else {
            None
        }
    }
}

/// `\\`, `\{`, `\}` and `\:` are recognized everywhere.
fn general(input: char) -> Option<char> {
    match input {
        CHAR_LITERAL_ESCAPE_CHAR => Some(CHAR_LITERAL_ESCAPE_CHAR),
        PLACEHOLDER_BEGIN_CHAR => Some(PLACEHOLDER_BEGIN_CHAR),
        PLACEHOLDER_END_CHAR => Some(PLACEHOLDER_END_CHAR),
        FORMATTER_NAME_SEPARATOR => Some(FORMATTER_NAME_SEPARATOR),
        _ => None,
    }
}

/// `\n`, `\t` … — only when `ParserSettings::convert_character_string_literals` is set.
fn character_literal(input: char) -> Option<char> {
    match input {
        '0' => Some('\0'),
        'a' => Some('\u{7}'),
        'b' => Some('\u{8}'),
        'f' => Some('\u{c}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'v' => Some('\u{b}'),
        _ => None,
    }
}

/// `\(` and `\)` — only inside formatter options.
fn formatter_option(input: char) -> Option<char> {
    match input {
        FORMATTER_OPTIONS_BEGIN_CHAR => Some(FORMATTER_OPTIONS_BEGIN_CHAR),
        FORMATTER_OPTIONS_END_CHAR => Some(FORMATTER_OPTIONS_END_CHAR),
        _ => None,
    }
}

/// The character an escape sequence `\<input>` stands for, if any.
pub(crate) fn try_get_char(
    input: char,
    include_formatter_option_chars: bool,
    include_character_literals: bool,
) -> Option<char> {
    general(input)
        .or_else(|| {
            include_character_literals
                .then(|| character_literal(input))
                .flatten()
        })
        .or_else(|| {
            include_formatter_option_chars
                .then(|| formatter_option(input))
                .flatten()
        })
}

/// Whether `input` is one of the characters .NET's number parser skips
/// (`Number.IsWhite`) for `NumberStyles.AllowLeadingWhite` /
/// `AllowTrailingWhite`: the space and the ASCII controls `0x09..=0x0D`.
/// Unicode whitespace is *not* included, so `\u\u{a0}123` is an error in .NET
/// as it is here, and so is `{Name:substr(\u{a0}1)}`.
///
/// Shared with `SubStringFormatter`, whose options go through the same
/// `int.Parse`.
pub(crate) fn is_dotnet_white(input: char) -> bool {
    input == ' ' || ('\u{9}'..='\u{d}').contains(&input)
}

/// The number the (up to) four characters of a `\uXXXX` sequence stand for,
/// parsed the way .NET's `int.TryParse(…, NumberStyles.HexNumber, …)` does:
/// leading and trailing whitespace is skipped, and neither a sign nor a `0x`
/// prefix is allowed. `\u 123` is therefore `0x123`, and `\u+123` an error.
fn parse_hex(digits: &[char]) -> Option<u16> {
    let start = digits
        .iter()
        .position(|&input| !is_dotnet_white(input))
        .unwrap_or(digits.len());
    let end = digits
        .iter()
        .rposition(|&input| !is_dotnet_white(input))
        .map_or(start, |last| last + 1);

    let body = &digits[start..end];
    if body.is_empty() {
        return None;
    }

    let mut value: u16 = 0;
    for &digit in body {
        let digit = u16::try_from(digit.to_digit(16)?).ok()?;
        // At most four hex digits are ever parsed, so this cannot overflow.
        value = value.checked_mul(16)?.checked_add(digit)?;
    }
    Some(value)
}

/// The UTF-16 code unit a `\uXXXX` sequence stands for. .NET casts the parsed
/// number to `char`, so the sequence may well be one half of a surrogate pair;
/// [`unescape`] joins the halves back together.
fn unicode(input: View<'_, '_>, start_index: usize) -> Result<u16, String> {
    // At most four characters are ever read, so they need no allocation.
    let mut digits = ['\0'; 4];
    let mut count = 0;
    while count < 4 {
        match input.get(start_index + count) {
            Some(digit) => {
                digits[count] = digit;
                count += 1;
            }
            None => break,
        }
    }
    let digits = &digits[..count];

    parse_hex(digits).ok_or_else(|| {
        let digits: String = digits.iter().collect();
        format!("Unrecognized escape sequence in literal: \"\\u{digits}\"")
    })
}

/// Whether `unit` is a high surrogate that can start a pair.
fn is_high_surrogate(unit: u16) -> bool {
    (0xd800..0xdc00).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xdc00..0xe000).contains(&unit)
}

/// The `\uXXXX` sequence starting at `index`, if there is one *and* it is a low
/// surrogate — the only sequence a high surrogate joins with.
fn low_surrogate_escape_at(input: View<'_, '_>, index: usize) -> Option<u16> {
    if input.get(index) != Some(CHAR_LITERAL_ESCAPE_CHAR) || input.get(index + 1) != Some('u') {
        return None;
    }
    unicode(input, index + 2)
        .ok()
        .filter(|&unit| is_low_surrogate(unit))
}

/// How many characters the `\uXXXX` sequence at `index` spans: 12 when a high
/// surrogate is followed by an escaped low surrogate, 6 otherwise.
///
/// .NET always takes 6 — the escape character, the `u` and four more
/// characters, whatever they are, clamped to the end of the input — and never
/// checks that those four are hex digits: `\u12}` is one literal of five
/// characters, closing brace included. It gets away with putting the two
/// halves of a surrogate pair into two literals because its output is UTF-16
/// and the halves meet again there; a Rust `String` cannot hold half a pair,
/// so the port keeps both halves in one literal instead and joins them in
/// [`unescape`]. That is the only place the two differ, and the parser pays
/// for it by resuming *inside* the sequence — the way .NET does — whenever
/// this returns 6; see `State::parse_alternative_escaping`.
pub(crate) fn unicode_escape_len(input: &Source<'_>, index: usize) -> usize {
    let input = input.all();
    let is_pair = unicode(input, index + 2).is_ok_and(is_high_surrogate)
        && low_surrogate_escape_at(input, index + 6).is_some();
    if is_pair {
        12
    } else {
        6
    }
}

/// Appends the character `units` stand for.
///
/// A lone surrogate has no `char`, and lands in the output as the replacement
/// character; .NET keeps it as an unpaired UTF-16 code unit, which a Rust
/// `String` cannot hold.
fn push_code_units(result: &mut String, units: &[u16]) {
    for decoded in char::decode_utf16(units.iter().copied()) {
        result.push(decoded.unwrap_or(char::REPLACEMENT_CHARACTER));
    }
}

/// The text of a literal, .NET `LiteralText.AsSpan()`: the characters as they
/// are, unless the literal *starts* with the escape character and the parser
/// resolves character literals, in which case its escape sequences are
/// resolved. `convert` is
/// [`ParserSettings::convert_character_string_literals`](super::ParserSettings::convert_character_string_literals).
///
/// `Ok(None)` means the text is `raw` unchanged — the common case, which .NET
/// answers with the untouched span and which the caller can answer by reusing
/// the string it already holds. Only a literal that really starts a sequence
/// allocates.
///
/// The parser gives every escape sequence a literal of its own, so this is
/// applied per sequence — and again to any slice of a literal a formatter's
/// [`Format::split`](super::Format::split) cuts, as .NET resolves a
/// `Format.Substring` slice afresh.
pub(crate) fn resolve_literal(raw: &str, convert: bool) -> Result<Option<String>, String> {
    if !raw.starts_with(CHAR_LITERAL_ESCAPE_CHAR) {
        return Ok(None);
    }
    if convert {
        // Only .NET's `span[0]` is looked at, so a sequence in the middle of a
        // literal is resolved as well — which is what `Format::split` hands
        // over when it cuts a literal the parser over-read.
        let span = Source::new(raw);
        return unescape(span.all(), false, true).map(Some);
    }
    // Special case: the escape character escaping itself, which .NET resolves
    // even with the conversion of character literals turned off.
    if raw.len() == 2 && raw.ends_with(CHAR_LITERAL_ESCAPE_CHAR) {
        return Ok(Some(CHAR_LITERAL_ESCAPE_CHAR.to_string()));
    }
    Ok(None)
}

/// Replaces escape sequences with the characters they stand for.
///
/// A trailing character that cannot start a sequence is copied verbatim, which
/// is what the .NET implementation does as well.
pub(crate) fn unescape(
    input: View<'_, '_>,
    include_formatter_option_chars: bool,
    include_character_literals: bool,
) -> Result<String, String> {
    let max = input.len();
    let mut result = String::with_capacity(max);
    let mut index = 0;

    while let Some(current) = input.get(index) {
        let Some(next) = input.get(index + 1) else {
            result.push(current);
            return Ok(result);
        };

        if current == CHAR_LITERAL_ESCAPE_CHAR {
            if next == 'u' {
                let unit = unicode(input, index + 2)?;
                index += 6;
                // A high surrogate takes a following escaped low surrogate with
                // it, the way the two code units join in a .NET string.
                let low = if is_high_surrogate(unit) {
                    low_surrogate_escape_at(input, index)
                } else {
                    None
                };
                match low {
                    Some(low) => {
                        index += 6;
                        push_code_units(&mut result, &[unit, low]);
                    }
                    None => push_code_units(&mut result, &[unit]),
                }
            } else if let Some(real) = try_get_char(
                next,
                include_formatter_option_chars,
                include_character_literals,
            ) {
                result.push(real);
                index += 2;
            } else {
                return Err(format!(
                    "Unrecognized escape sequence \"{current}{next}\" in literal."
                ));
            }
        } else {
            result.push(current);
            index += 1;
        }
    }

    Ok(result)
}
