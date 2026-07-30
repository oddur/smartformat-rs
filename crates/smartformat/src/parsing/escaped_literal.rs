//! Escape sequences inside literal text and formatter options.
//!
//! Ported from SmartFormat.NET `src/SmartFormat/Core/Parsing/EscapedLiteral.cs`.

use super::chars::CHAR_LITERAL_ESCAPE_CHAR;

/// `\\`, `\{`, `\}` and `\:` are recognized everywhere.
fn general(input: char) -> Option<char> {
    match input {
        '\\' => Some('\\'),
        '{' => Some('{'),
        '}' => Some('}'),
        ':' => Some(':'),
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
        '(' => Some('('),
        ')' => Some(')'),
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

fn unicode(input: &[char], start_index: usize) -> Result<char, String> {
    let end = (start_index + 4).min(input.len());
    let digits: String = input
        .get(start_index..end)
        .unwrap_or_default()
        .iter()
        .collect();
    u32::from_str_radix(&digits, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| format!("Unrecognized escape sequence in literal: \"\\u{digits}\""))
}

/// Replaces escape sequences with the characters they stand for.
///
/// A trailing character that cannot start a sequence is copied verbatim, which
/// is what the .NET implementation does as well.
pub(crate) fn unescape(
    input: &[char],
    include_formatter_option_chars: bool,
    include_character_literals: bool,
) -> Result<String, String> {
    let max = input.len();
    let mut result = String::with_capacity(max);
    let mut index = 0;

    while index < max {
        if index + 1 >= max {
            result.push(input[index]);
            return Ok(result);
        }

        if input[index] == CHAR_LITERAL_ESCAPE_CHAR {
            if input[index + 1] == 'u' {
                result.push(unicode(input, index + 2)?);
                index += 6;
            } else if let Some(real) = try_get_char(
                input[index + 1],
                include_formatter_option_chars,
                include_character_literals,
            ) {
                result.push(real);
                index += 2;
            } else {
                return Err(format!(
                    "Unrecognized escape sequence \"{}{}\" in literal.",
                    input[index],
                    input[index + 1]
                ));
            }
        } else {
            result.push(input[index]);
            index += 1;
        }
    }

    Ok(result)
}
