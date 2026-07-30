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

/// The UTF-16 code unit a `\uXXXX` sequence stands for. .NET casts the parsed
/// number to `char`, so the sequence may well be one half of a surrogate pair;
/// [`unescape`] joins the halves back together.
pub(crate) fn unicode(input: &[char], start_index: usize) -> Result<u16, String> {
    let end = (start_index + 4).min(input.len());
    let digits: String = input
        .get(start_index..end)
        .unwrap_or_default()
        .iter()
        .collect();
    u32::from_str_radix(&digits, 16)
        .ok()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| format!("Unrecognized escape sequence in literal: \"\\u{digits}\""))
}

/// The `\uXXXX` sequence starting at `index`, if there is one.
fn unicode_escape_at(input: &[char], index: usize) -> Option<u16> {
    if input.get(index) != Some(&CHAR_LITERAL_ESCAPE_CHAR) || input.get(index + 1) != Some(&'u') {
        return None;
    }
    unicode(input, index + 2).ok()
}

/// Whether `unit` is a high surrogate that can start a pair.
pub(crate) fn is_high_surrogate(unit: u16) -> bool {
    (0xd800..0xdc00).contains(&unit)
}

fn is_low_surrogate(unit: u16) -> bool {
    (0xdc00..0xe000).contains(&unit)
}

/// The character `unit` stands for, or, for the second half of a pair, the
/// character `unit` and `low` stand for together.
///
/// A lone surrogate has no `char`, and lands in the output as the replacement
/// character; .NET keeps it as an unpaired UTF-16 code unit, which a Rust
/// `String` cannot hold.
fn from_code_units(unit: u16, low: Option<u16>) -> char {
    match low {
        Some(low) => {
            let code = 0x1_0000 + ((u32::from(unit) - 0xd800) << 10) + (u32::from(low) - 0xdc00);
            char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER)
        }
        None => char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER),
    }
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
                let unit = unicode(input, index + 2)?;
                index += 6;
                // A high surrogate takes the following `\uXXXX` with it, the
                // way the two code units join in a .NET string.
                let low = if is_high_surrogate(unit) {
                    unicode_escape_at(input, index).filter(|&next| is_low_surrogate(next))
                } else {
                    None
                };
                if low.is_some() {
                    index += 6;
                }
                result.push(from_code_units(unit, low));
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
