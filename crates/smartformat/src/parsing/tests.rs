//! Ported from SmartFormat.NET `src/SmartFormat.Tests/Core/ParserTests.cs`.

use super::parser::{
    INVALID_CHARACTERS_IN_SELECTOR, MISSING_CLOSING_BRACE, TOO_MANY_CLOSING_BRACES,
    TRAILING_OPERATORS_IN_SELECTOR,
};
use super::{Format, FormatItem, Parser, ParserSettings, Placeholder, SelectorFilter};
use crate::error::Error;
use crate::settings::ErrorAction;

fn parser() -> Parser {
    Parser::default()
}

fn parser_with(settings: ParserSettings) -> Parser {
    Parser::new(settings)
}

fn no_convert() -> ParserSettings {
    ParserSettings {
        convert_character_string_literals: false,
        ..ParserSettings::default()
    }
}

fn parse(format: &str) -> Format {
    parser()
        .parse(format)
        .unwrap_or_else(|e| panic!("failed to parse {format:?}: {e}"))
}

fn errors(result: Result<Format, Error>) -> Vec<crate::error::ParseError> {
    match result {
        Err(Error::Parse { errors }) => errors,
        Err(other) => panic!("expected a parse error, got {other}"),
        Ok(format) => panic!("expected a parse error, got {format:?}"),
    }
}

fn first_placeholder(format: &Format) -> &Placeholder {
    match &format.items[0] {
        FormatItem::Placeholder(placeholder) => placeholder,
        other => panic!("expected a placeholder, got {other:?}"),
    }
}

fn placeholder_at(format: &Format, index: usize) -> &Placeholder {
    match &format.items[index] {
        FormatItem::Placeholder(placeholder) => placeholder,
        other => panic!("expected a placeholder at {index}, got {other:?}"),
    }
}

// ----- round trips --------------------------------------------------------

#[test]
fn basic_parser_test() {
    let formats = [
        "{City}{PostalCode}{Gender}{FirstName}{LastName}",
        "Address: {City.ZipCode} {City.Name}, {City.AreaCode}\nName: {Person.FirstName} {Person.LastName}",
        "{a.b.c.d}",
        " aaa {bbb.ccc: ddd {eee} fff } ggg ",
        "{aaa} {bbb}",
        "{}",
        "{a:{b:{c:{d}}}}",
        "{a}",
        " aaa {bbb_bbb.CCC} ddd ",
    ];

    for format in formats {
        let parsed = parse(format);
        assert_eq!(parsed.to_string(), format, "round trip of {format:?}");
    }
}

#[test]
fn spans_cover_the_input() {
    let parsed = parse(" aaa {bbb.ccc: ddd {eee} fff } ggg ");
    let input = " aaa {bbb.ccc: ddd {eee} fff } ggg ";

    assert_eq!(parsed.items.len(), 3);
    assert_eq!(
        &input[parsed.items[0].start()..parsed.items[0].end()],
        " aaa "
    );
    assert_eq!(
        &input[parsed.items[1].start()..parsed.items[1].end()],
        "{bbb.ccc: ddd {eee} fff }"
    );
    assert_eq!(
        &input[parsed.items[2].start()..parsed.items[2].end()],
        " ggg "
    );
    assert_eq!(parsed.items[1].raw(), "{bbb.ccc: ddd {eee} fff }");
}

// ----- errors -------------------------------------------------------------

#[test]
fn parser_returns_errors() {
    let invalid = [
        "{", "{0", "}", "0}", "{{{", "}}}", "{.}", "{.:}", "{..}", "{..:}", "{0.}", "{0.:}",
    ];

    for format in invalid {
        let result = parser().parse(format);
        assert!(
            result.is_err(),
            "{format:?} should not parse, got {:?}",
            result.ok()
        );
    }
}

#[test]
fn parser_returns_errors_for_illegal_selector_chars() {
    // Braces are not allowed, the escape char is not allowed, and a trailing
    // operator ends the selector illegally.
    for format in ["{V(LU)}", "{V LU\\}", "{V?LU,}"] {
        let issues = errors(parser().parse(format));
        assert!(!issues.is_empty(), "{format:?} should report issues");
    }
}

#[test]
fn error_messages_name_the_problem() {
    let issues = errors(parser().parse("{0:yyyy/MM/dd HH:mm:ss"));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].message, MISSING_CLOSING_BRACE);

    let issues = errors(parser().parse("0}"));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].message, TOO_MANY_CLOSING_BRACES);
    assert_eq!(issues[0].position, 1);

    let issues = errors(parser().parse("{0.}"));
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.ends_with(TRAILING_OPERATORS_IN_SELECTOR));
    assert_eq!(issues[0].position, 2);

    let issues = errors(parser().parse("{a b}"));
    assert_eq!(issues.len(), 1);
    assert_eq!(
        issues[0].message,
        format!("'0x20': {INVALID_CHARACTERS_IN_SELECTOR}")
    );
    assert_eq!(issues[0].position, 2);
}

#[test]
fn error_count_matches_dotnet() {
    assert_eq!(errors(parser().parse("{.}")).len(), 1);
    assert_eq!(errors(parser().parse("{.}{0.}")).len(), 2);
    // Two illegal spaces, one illegal '{', one missing closing brace.
    assert_eq!(errors(parser().parse("{NoName {Other} {Same")).len(), 3);
    assert_eq!(
        errors(parser().parse("Hello, I'm {Name from {City}")).len(),
        3
    );
}

#[test]
fn error_positions_are_utf16_offsets() {
    // 'ä' is two UTF-8 bytes but one UTF-16 code unit, and .NET counts the
    // latter: the space sits at index 5, not at byte 7.
    let issues = errors(parser().parse("äöü{a b}"));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].position, 5);
    assert_ne!(issues[0].position, "äöü{a".len());

    // An astral character is two UTF-16 code units but one `char`.
    let issues = errors(parser().parse("\u{1f600}{a b}"));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].position, 4);
}

#[test]
fn parser_ignores_errors() {
    let settings = ParserSettings {
        error_action: ErrorAction::Ignore,
        ..ParserSettings::default()
    };
    let parser = parser_with(settings);

    for format in [
        "{", "{0", "}", "0}", "{{{", "}}}", "{.}", "{.:}", "{..}", "{..:}", "{0.}", "{0.:}",
    ] {
        assert!(parser.parse(format).is_ok(), "{format:?} should not fail");
    }
}

#[test]
fn error_action_ignore_drops_the_placeholder() {
    //                    | literal  | erroneous     | | okay  |
    let invalid_template = "Hello, I'm {Name from {City} {Street}";
    let parser = parser_with(ParserSettings {
        error_action: ErrorAction::Ignore,
        ..ParserSettings::default()
    });

    let parsed = parser.parse(invalid_template).unwrap();

    assert_eq!(parsed.items.len(), 4);
    assert_eq!(parsed.items[0].raw(), "Hello, I'm ");
    assert_eq!(parsed.items[1].raw(), "");
    assert_eq!(parsed.items[2].raw(), " ");
    assert_eq!(parsed.items[3].raw(), "{Street}");
    assert!(matches!(parsed.items[3], FormatItem::Placeholder(_)));
}

#[test]
fn error_action_maintain_tokens_keeps_the_text() {
    let parser = parser_with(ParserSettings {
        error_action: ErrorAction::MaintainTokens,
        ..ParserSettings::default()
    });

    for (template, last_item_is_placeholder) in [
        ("Hello, I'm {Name from {City} {Street}", true),
        ("Hello, I'm {Name from {City} {Street", false),
    ] {
        let parsed = parser.parse(template).unwrap();

        assert_eq!(parsed.items.len(), 4, "item count of {template:?}");
        assert_eq!(parsed.items[0].raw(), "Hello, I'm ");
        assert_eq!(parsed.items[1].raw(), "{Name from {City}");
        assert_eq!(parsed.items[2].raw(), " ");

        if last_item_is_placeholder {
            assert!(matches!(parsed.items[3], FormatItem::Placeholder(_)));
            assert_eq!(parsed.items[3].raw(), "{Street}");
        } else {
            assert!(matches!(parsed.items[3], FormatItem::Literal(_)));
            assert_eq!(parsed.items[3].raw(), "{Street");
        }
        // Nothing is lost, whatever the recovery.
        let restored: String = parsed.items.iter().map(|item| item.raw()).collect();
        assert_eq!(restored, template);
    }
}

#[test]
fn error_carets_are_positioned_in_utf16_code_units() {
    let parser = parser_with(ParserSettings {
        error_action: ErrorAction::OutputErrorInResult,
        ..ParserSettings::default()
    });

    // Every one of these messages is what SmartFormat.NET 3.6.1 produces.
    for (template, expected) in [
        (
            "äöü{a b}",
            concat!(
                "The format string has 1 issue:\n",
                "'0x20': Invalid character in the selector\n",
                "In: \"äöü{a b}\"\n",
                "At:  -----^ "
            ),
        ),
        (
            "\u{1f600}{a b}",
            concat!(
                "The format string has 1 issue:\n",
                "'0x20': Invalid character in the selector\n",
                "In: \"\u{1f600}{a b}\"\n",
                "At:  ----^ "
            ),
        ),
        (
            "\u{1f600}0}",
            concat!(
                "The format string has 1 issue:\n",
                "Format string has too many closing braces\n",
                "In: \"\u{1f600}0}\"\n",
                "At:  ---^ "
            ),
        ),
    ] {
        assert_eq!(
            parser.parse(template).unwrap().to_string(),
            expected,
            "message for {template:?}"
        );
    }
}

#[test]
fn error_action_output_error_in_result() {
    let parser = parser_with(ParserSettings {
        error_action: ErrorAction::OutputErrorInResult,
        ..ParserSettings::default()
    });

    let parsed = parser.parse("Hello, I'm {Name from {City}").unwrap();

    assert_eq!(parsed.items.len(), 1);
    assert!(
        parsed.items[0]
            .raw()
            .starts_with("The format string has 3 issues"),
        "unexpected message: {}",
        parsed.items[0].raw()
    );
}

// ----- selectors and alignment -------------------------------------------

#[test]
fn format_with_alignment() {
    let parsed = parse("{0,-10}");
    let placeholder = first_placeholder(&parsed);

    assert_eq!(placeholder.selectors.len(), 2);
    assert_eq!(placeholder.selectors[0].text, "0");
    assert_eq!(placeholder.selectors[1].operator, ",");
    assert_eq!(placeholder.selectors[1].text, "-10");
    assert_eq!(placeholder.alignment, -10);
    assert_eq!(placeholder.to_string(), "{0,-10}");
}

#[test]
fn nested_placeholders_inherit_the_alignment() {
    let parsed = parse("{0,-10:{1}}");
    let outer = first_placeholder(&parsed);
    assert_eq!(outer.alignment, -10);

    let inner = first_placeholder(outer.format.as_ref().unwrap());
    assert_eq!(inner.alignment, -10);
    assert_eq!(inner.nested_depth, 2);
    assert_eq!(outer.nested_depth, 1);
}

#[test]
fn selector_chain() {
    let parsed = parse("{Person.Birthday.Year}");
    let placeholder = first_placeholder(&parsed);

    let texts: Vec<&str> = placeholder
        .selectors
        .iter()
        .map(|selector| selector.text.as_str())
        .collect();
    assert_eq!(texts, ["Person", "Birthday", "Year"]);
    assert_eq!(placeholder.selectors[0].operator, "");
    assert_eq!(placeholder.selectors[1].operator, ".");
    assert_eq!(placeholder.selectors[2].index, 2);
}

#[test]
fn nameless_placeholder_has_no_selectors() {
    let parsed = parse("{}");
    let placeholder = first_placeholder(&parsed);
    assert!(placeholder.selectors.is_empty());
    assert!(placeholder.format.is_none());
}

#[test]
fn numeric_index_selectors() {
    let parsed = parse("{Numbers[0].Length}");
    let placeholder = first_placeholder(&parsed);

    let selectors: Vec<(&str, &str)> = placeholder
        .selectors
        .iter()
        .map(|selector| (selector.operator.as_str(), selector.text.as_str()))
        .collect();
    assert_eq!(selectors, [("", "Numbers"), ("[", "0"), ("].", "Length")]);
}

#[test]
fn selector_with_nullable_operator() {
    // Contiguous operator characters are parsed as one operator.
    let parsed = parse("{A?.B}");
    let placeholder = first_placeholder(&parsed);
    assert_eq!(placeholder.selectors.len(), 2);
    assert_eq!(placeholder.selectors[0].text, "A");
    assert_eq!(placeholder.selectors[1].operator, "?.");
    assert_eq!(placeholder.selectors[1].text, "B");

    let parsed = parse("{List?[123].Selector}");
    let placeholder = first_placeholder(&parsed);
    assert_eq!(placeholder.selectors.len(), 3);
    assert_eq!(placeholder.selectors[0].text, "List");
    assert_eq!(placeholder.selectors[1].operator, "?[");
    assert_eq!(placeholder.selectors[1].text, "123");
    assert_eq!(placeholder.selectors[2].operator, "].");
    assert_eq!(placeholder.selectors[2].text, "Selector");
}

#[test]
fn trailing_list_index_adds_an_empty_selector() {
    // ']' right before '}' closes the list index instead of being a trailing operator.
    let parsed = parse("{Numbers[0]}");
    let placeholder = first_placeholder(&parsed);
    assert_eq!(placeholder.selectors.len(), 3);
    assert_eq!(placeholder.selectors[2].operator, "]");
    assert_eq!(placeholder.selectors[2].text, "");
}

#[test]
fn selector_with_custom_selector_character() {
    for (format, custom) in [("{A }", ' '), ("{B§}", '§'), ("{%C}", '%')] {
        let mut settings = ParserSettings::default();
        settings.add_custom_selector_chars([custom]).unwrap();
        let parsed = parser_with(settings).parse(format).unwrap();

        let placeholder = first_placeholder(&parsed);
        assert_eq!(placeholder.selectors.len(), 1);
        assert_eq!(
            placeholder.selectors[0].text,
            format.chars().skip(1).take(2).collect::<String>()
        );
    }
}

#[test]
fn selectors_with_custom_operator_character() {
    for (format, custom) in [("{a b}", ' '), ("{a°b}", '°')] {
        let mut settings = ParserSettings::default();
        settings.add_custom_operator_chars([custom]).unwrap();
        let parsed = parser_with(settings).parse(format).unwrap();

        let placeholder = first_placeholder(&parsed);
        assert_eq!(placeholder.selectors.len(), 2);
        assert_eq!(placeholder.selectors[0].text, "a");
        assert_eq!(placeholder.selectors[1].text, "b");
        assert_eq!(placeholder.selectors[1].operator, custom.to_string());
    }
}

#[test]
fn selector_with_contiguous_operator_characters() {
    for (format, custom) in [("{A?.B}", '.'), ("{C%.D}", '%'), ("{C..D}", '.')] {
        let mut settings = ParserSettings::default();
        settings.add_custom_operator_chars([custom]).unwrap();
        let parsed = parser_with(settings).parse(format).unwrap();

        let placeholder = first_placeholder(&parsed);
        assert_eq!(placeholder.selectors.len(), 2, "for {format:?}");
        let chars: Vec<char> = format.chars().collect();
        assert_eq!(placeholder.selectors[0].text, chars[1].to_string());
        assert_eq!(placeholder.selectors[1].text, chars[4].to_string());
        assert_eq!(
            placeholder.selectors[1].operator,
            chars[2..4].iter().collect::<String>()
        );
    }
}

#[test]
fn adding_a_reserved_operator_char_as_selector_char_fails() {
    let mut settings = ParserSettings::default();
    assert!(settings.add_custom_selector_chars(['.']).is_err());
    assert!(settings.add_custom_selector_chars(['\\']).is_err());
    assert!(settings.add_custom_selector_chars([':']).is_err());
}

#[test]
fn selector_works_with_all_unicode_chars() {
    let settings = ParserSettings {
        selector_char_filter: SelectorFilter::VisualUnicodeChars,
        ..ParserSettings::default()
    };
    let parser = parser_with(settings);

    for selector in [
        "German |öäüßÖÄÜ!",
        "Russian абвгдеёжзийклмн",
        "French >éèêëçàùâîô",
        "Chinese 汉字测试",
        "Arabic مرحبا بالعالم",
    ] {
        let template = format!("{{{selector}}}");
        let parsed = parser
            .parse(&template)
            .unwrap_or_else(|e| panic!("failed to parse {template:?}: {e}"));
        let placeholder = first_placeholder(&parsed);
        assert_eq!(placeholder.selectors.len(), 1);
        assert_eq!(placeholder.selectors[0].text, selector);
    }
}

#[test]
fn parsing_selector_with_char_from_blocklist_fails() {
    let settings = ParserSettings {
        selector_char_filter: SelectorFilter::VisualUnicodeChars,
        ..ParserSettings::default()
    };

    // The newline character is in the default blocklist.
    let issues = errors(parser_with(settings).parse("{A\nB}"));
    assert!(issues
        .iter()
        .any(|issue| issue.message.ends_with(INVALID_CHARACTERS_IN_SELECTOR)));
}

// ----- formatter name and options ----------------------------------------

#[test]
fn name_of_named_formatter_is_parsed() {
    let cases = [
        ("{0:name:}", "name", "", ""),
        ("{0:name()}", "name", "", ""),
        ("{0:name(1|2|3)}", "name", "1|2|3", ""),
        ("{0:name:format}", "name", "", "format"),
        ("{0:name():format}", "name", "", "format"),
        ("{0:name():}", "name", "", ""),
        ("{0:name(1|2|3):format}", "name", "1|2|3", "format"),
        ("{0:name(1|2|3):}", "name", "1|2|3", ""),
    ];

    for (format, name, options, nested) in cases {
        let parsed = parse(format);
        let placeholder = first_placeholder(&parsed);

        assert_eq!(placeholder.formatter_name, name, "name of {format:?}");
        assert_eq!(
            placeholder.formatter_options, options,
            "options of {format:?}"
        );
        assert_eq!(
            placeholder.format.as_ref().unwrap().to_string(),
            nested,
            "format of {format:?}"
        );
    }
}

#[test]
fn named_formatter_is_empty_when_invalid_or_escaped() {
    let cases = [
        // Incomplete:
        r"{0:format}",
        r"{0:format(}",
        r"{0:format)}",
        r"{0:(format)}",
        // Invalid:
        r"{0:format()stuff}",
        r"{0:format() :}",
        r"{0:format(s)|stuff}",
        r"{0:format(s)stuff:}",
        r"{0:format(:}",
        r"{0:format):}",
        // Escape sequences:
        r"{0:format\()}",
        r"{0:format(\)}",
        r"{0:format\:}",
        r"{0:hh\:mm\:ss}",
        // Empty:
        r"{0:}",
        r"{0::}",
        r"{0:()}",
        r"{0:():}",
        r"{0:(1|2|3)}",
        r"{0:(1|2|3):}",
    ];

    let parser = parser_with(no_convert());

    for format in cases {
        let expected: String = format
            .chars()
            .skip(3)
            .take(format.chars().count() - 4)
            .collect();
        let parsed = parser
            .parse(format)
            .unwrap_or_else(|e| panic!("failed to parse {format:?}: {e}"));
        let placeholder = first_placeholder(&parsed);

        assert_eq!(placeholder.formatter_name, "", "name of {format:?}");
        assert_eq!(placeholder.formatter_options, "", "options of {format:?}");
        assert_eq!(
            placeholder.format.as_ref().unwrap().literal_text(),
            expected,
            "literal text of {format:?}"
        );
    }
}

#[test]
fn named_formatter_is_empty_when_the_format_has_nesting() {
    let cases = [
        (r"{0:format{}}", "format"),
        (r"{0:{}}", ""),
        (r"{0:{0:nested():}}", ""),
        (r"{0:for{}mat}", "format"),
        (r"{0:for{}mat()}", "format()"),
        (r"{0:for(){}mat}", "for()mat"),
    ];

    let parser = parser_with(no_convert());

    for (format, expected) in cases {
        let parsed = parser.parse(format).unwrap();
        let placeholder = first_placeholder(&parsed);

        assert_eq!(placeholder.formatter_name, "", "name of {format:?}");
        assert_eq!(placeholder.formatter_options, "", "options of {format:?}");
        assert_eq!(
            placeholder.format.as_ref().unwrap().literal_text(),
            expected,
            "literal text of {format:?}"
        );
    }
}

#[test]
fn parse_options() {
    let selector = "0";
    let formatter_name = "c";
    // Unescaped {}:() finish the options; unescaped operators []., are fine.
    let options = r"\{.\)\:,_][1|2|3";
    // The literal may contain escaped characters too.
    let literal = r"one|two|th\} \{ree|other";

    let format = format!("{{{selector}:{formatter_name}({options}):{literal}}}");
    let parsed = parse(&format);
    let placeholder = first_placeholder(&parsed);

    assert_eq!(parsed.items.len(), 1);
    assert!(parsed.has_nested());
    assert_eq!(placeholder.selectors[0].text, selector);
    assert_eq!(placeholder.formatter_name, formatter_name);
    assert_eq!(placeholder.formatter_options, options.replace('\\', ""));
    assert_eq!(placeholder.formatter_options_raw, options);

    let nested = placeholder.format.as_ref().unwrap();
    assert_eq!(nested.items.len(), 3);
    assert_eq!(nested.literal_text(), literal.replace('\\', ""));
}

#[test]
fn nested_format_with_literal_escaping() {
    let parsed = parse("{c1:{c2:{c3}}}");

    let c1 = first_placeholder(&parsed);
    let c2 = first_placeholder(c1.format.as_ref().unwrap());
    let c3 = first_placeholder(c2.format.as_ref().unwrap());

    assert_eq!(c1.selectors[0].text, "c1");
    assert_eq!(c2.selectors[0].text, "c2");
    assert_eq!(c3.selectors[0].text, "c3");
    assert_eq!(c3.nested_depth, 3);
}

// ----- escaping -----------------------------------------------------------

#[test]
fn literal_escaping_in_literal() {
    for convert in [true, false] {
        let parser = parser_with(ParserSettings {
            convert_character_string_literals: convert,
            ..ParserSettings::default()
        });
        assert_eq!(parser.parse(r"\{\}").unwrap().to_string(), "{}");
    }
}

#[test]
fn escaping_the_escaping_character() {
    // https://github.com/axuno/SmartFormat/issues/493
    let parser = parser_with(no_convert());
    let input = r"\\\\aaa\\\{\}bbb ccc\x\{\}ddd\\\\";

    assert_eq!(
        parser.parse(input).unwrap().to_string(),
        r"\\aaa\{}bbb ccc\x{}ddd\\"
    );
}

#[test]
fn character_literals_are_converted_by_default() {
    let parsed = parse(r"a\tb\nc");
    assert_eq!(parsed.to_string(), "a\tb\nc");
    // Every escape sequence gets a literal item of its own.
    assert_eq!(parsed.items.len(), 5);
    assert_eq!(parsed.items[1].raw(), r"\t");
}

#[test]
fn escaped_braces_are_not_placeholders() {
    let parsed = parse(r"a\{b\}c");
    assert_eq!(parsed.to_string(), "a{b}c");
    assert!(!parsed.has_nested());
}

#[test]
fn parse_unicode() {
    // The literal keeps the whole escape sequence, and resolves it.
    for (format, literal, item_index, expected) in [
        (r"\u1234", r"\u1234", 0, '\u{1234}'),
        (r"\u1234abc", r"\u1234", 0, '\u{1234}'),
        (r"abc\u1234", r"\u1234", 1, '\u{1234}'),
        (r"abc\u1234def", r"\u1234", 1, '\u{1234}'),
    ] {
        let parsed = parse(format);
        let item = &parsed.items[item_index];
        assert_eq!(item.raw(), literal, "raw text of {format:?}");
        match item {
            FormatItem::Literal(text) => {
                assert_eq!(text.text.chars().next(), Some(expected))
            }
            other => panic!("expected literal text, got {other:?}"),
        }
    }

    // .NET accepts the illegal sequence while parsing and throws when the
    // literal is rendered; this port owns its strings, so it fails right here.
    for format in [r"\uwxyz", r"\uw"] {
        assert!(parser().parse(format).is_err(), "{format:?} should fail");
    }
}

#[test]
fn parse_unicode_surrogate_pair() {
    // .NET spells an astral character as two UTF-16 code units, which end up
    // in two adjacent literals and concatenate in the output string; here the
    // pair goes into one literal instead, which is the only way a Rust
    // `String` can hold it.
    let parsed = parse(r"a\ud83d\ude00b");
    assert_eq!(parsed.literal_text(), "a\u{1f600}b");
    assert_eq!(parsed.items.len(), 3);
    assert_eq!(parsed.items[1].raw(), r"\ud83d\ude00");

    assert_eq!(parse(r"\ud83d\ude00").literal_text(), "\u{1f600}");

    // An unpaired surrogate has no `char`. .NET keeps it as a code unit; the
    // closest a `String` gets is the replacement character.
    assert_eq!(parse(r"a\ud800b").literal_text(), "a\u{fffd}b");
    assert_eq!(parse(r"a\ude00b").literal_text(), "a\u{fffd}b");
    assert_eq!(parse(r"a\ud800Ab").literal_text(), "a\u{fffd}\u{41}b");
}

#[test]
fn high_surrogate_pairs_only_with_a_low_surrogate() {
    // A lone high surrogate followed by a *complete* pair: the lone unit must
    // not swallow the high half of the pair, or the emoji straddles a literal
    // boundary and three replacement characters come out. .NET writes the
    // unpaired code unit and then the emoji.
    let parsed = parse(r"a\ud83d\ud83d\ude00");
    assert_eq!(parsed.literal_text(), "a\u{fffd}\u{1f600}");
    assert_eq!(parsed.items.len(), 3);
    assert_eq!(parsed.items[1].raw(), r"\ud83d");
    assert_eq!(parsed.items[2].raw(), r"\ud83d\ude00");

    // A low surrogate never starts a pair.
    let parsed = parse(r"a\ude00\ud83d\ude00");
    assert_eq!(parsed.literal_text(), "a\u{fffd}\u{1f600}");
    assert_eq!(parsed.items.len(), 3);
    assert_eq!(parsed.items[2].raw(), r"\ud83d\ude00");

    // Nor does a high surrogate pair with an escape that is not a surrogate.
    let parsed = parse(r"a\ud83d\u0041b");
    assert_eq!(parsed.literal_text(), "a\u{fffd}Ab");
    assert_eq!(parsed.items[1].raw(), r"\ud83d");
    assert_eq!(parsed.items[2].raw(), r"\u0041");
}

#[test]
fn unicode_escape_parses_hex_the_way_dotnet_does() {
    // .NET parses the four characters with `NumberStyles.HexNumber`, which
    // skips leading and trailing whitespace — space and 0x09..=0x0D only.
    assert_eq!(parse(r"\u 123").literal_text(), "\u{123}");
    assert_eq!(parse(r"x\u 123y").literal_text(), "x\u{123}y");
    assert_eq!(parse(r"\u  12").literal_text(), "\u{12}");
    assert_eq!(parse("\\u123\tz").literal_text(), "\u{123}z");
    // A window shortened by the end of the input is parsed as it stands.
    assert_eq!(parse(r"abc\u12").literal_text(), "abc\u{12}");

    // `NumberStyles.HexNumber` allows neither a sign nor a `0x` prefix, and
    // whitespace only around the digits.
    for template in [
        r"\u+123",
        r"\u-123",
        r"\u0x12",
        r"\u12 3",
        r"\u    ",
        r"abc\u",
        "\\u\u{a0}123",
    ] {
        assert!(
            parser().parse(template).is_err(),
            "{template:?} should not parse"
        );
    }
}

#[test]
fn placeholder_display_rebuilds_the_placeholder() {
    // .NET `Placeholder.ToString()` normalizes the alignment, unescapes the
    // formatter options, and always ends a non-null format with a colon —
    // but writes the format itself as the raw source text.
    for (template, expected) in [
        ("{0}", "{0}"),
        ("{0:D3}", "{0:D3}"),
        ("{Missing,05}", "{Missing,5}"),
        ("{Missing,-05}", "{Missing,-5}"),
        (r"{Missing:d(a\:b)}", "{Missing:d(a:b):}"),
        (r"{Missing,05:d(a\:b)}", "{Missing,5:d(a:b):}"),
        ("{Missing:d()}", "{Missing:d:}"),
        (r"{Missing:\n}", r"{Missing:\n}"),
        ("{A.B?.C}", "{A.B?.C}"),
    ] {
        let parsed = parse(template);
        assert_eq!(
            first_placeholder(&parsed).to_string(),
            expected,
            "template {template:?}"
        );
    }
}

#[test]
fn escape_sequence_at_the_end_of_the_input_fails() {
    assert!(parser().parse(r"abc\").is_err());
}

#[test]
fn unrecognized_escape_sequence_fails_when_converting() {
    let issues = errors(parser().parse(r"abc\xyz"));
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains(r"\x"));
    // The position is the escape character, and the caret spans the sequence.
    assert_eq!(issues[0].position, 3);
}

#[test]
fn unrecognized_escape_sequences_obey_the_error_action() {
    // Every escape error is an issue like any other, so the error action
    // decides what happens to it — the parser never fails outright unless the
    // action says so.
    let templates = [
        r"abc\xyz",         // unknown sequence in literal text
        r"abc\",            // escape character at the very end
        r"a\uwxyz",         // unparsable \u sequence
        r"{0:d(a\qb):txt}", // unknown sequence in formatter options
    ];

    for template in templates {
        assert!(
            parser().parse(template).is_err(),
            "{template:?} should fail with ErrorAction::Error"
        );

        for action in [
            ErrorAction::Ignore,
            ErrorAction::MaintainTokens,
            ErrorAction::OutputErrorInResult,
        ] {
            let parsed = parser_with(ParserSettings {
                error_action: action,
                ..ParserSettings::default()
            })
            .parse(template);
            assert!(
                parsed.is_ok(),
                "{template:?} should recover with {action:?}, got {:?}",
                parsed.err()
            );
        }
    }
}

#[test]
fn recovered_escape_sequences_stay_as_written() {
    // Outside a placeholder there is nothing to drop, so the sequence itself
    // is what stays in the output, whichever lenient action is chosen.
    for action in [ErrorAction::Ignore, ErrorAction::MaintainTokens] {
        let parser = parser_with(ParserSettings {
            error_action: action,
            ..ParserSettings::default()
        });
        assert_eq!(parser.parse(r"abc\xyz").unwrap().to_string(), r"abc\xyz");
        assert_eq!(parser.parse(r"abc\").unwrap().to_string(), r"abc\");
    }

    // Inside a placeholder the ordinary recovery applies.
    let template = r"{0:d(a\qb)}";
    let dropped = parser_with(ParserSettings {
        error_action: ErrorAction::Ignore,
        ..ParserSettings::default()
    })
    .parse(template)
    .unwrap();
    assert_eq!(dropped.to_string(), "");

    let kept = parser_with(ParserSettings {
        error_action: ErrorAction::MaintainTokens,
        ..ParserSettings::default()
    })
    .parse(template)
    .unwrap();
    assert_eq!(kept.to_string(), template);
}

#[test]
fn escape_errors_are_reported_like_other_issues() {
    let parsed = parser_with(ParserSettings {
        error_action: ErrorAction::OutputErrorInResult,
        ..ParserSettings::default()
    })
    .parse(r"abc\xyz")
    .unwrap();

    assert_eq!(
        parsed.to_string(),
        concat!(
            "The format string has 1 issue:\n",
            "Unrecognized escape sequence \"\\x\" in literal.\n",
            "In: \"abc\\xyz\"\n",
            "At:  ---^^ "
        )
    );
}

#[test]
fn string_format_escaping_in_literal() {
    let parser = parser_with(ParserSettings {
        string_format_compatibility: true,
        ..ParserSettings::default()
    });
    assert_eq!(parser.parse("{{}}").unwrap().to_string(), "{}");
}

// ----- structure ----------------------------------------------------------

#[test]
fn placeholders_and_literals_are_in_source_order() {
    let parsed = parse(" aaa {bbb: ccc {ddd} eee } fff {ggg} ");

    assert_eq!(parsed.items.len(), 5);
    assert!(matches!(parsed.items[0], FormatItem::Literal(_)));
    assert!(matches!(parsed.items[1], FormatItem::Placeholder(_)));
    assert!(matches!(parsed.items[2], FormatItem::Literal(_)));
    assert!(matches!(parsed.items[3], FormatItem::Placeholder(_)));
    assert!(matches!(parsed.items[4], FormatItem::Literal(_)));

    let nested = placeholder_at(&parsed, 1).format.as_ref().unwrap();
    assert_eq!(nested.items.len(), 3);
    assert_eq!(nested.to_string(), " ccc {ddd} eee ");
    assert_eq!(placeholder_at(nested, 1).selectors[0].text, "ddd");
}

#[test]
fn nested_format_start_is_after_the_formatter_name() {
    let input = "{0:default:N2}";
    let parsed = parse(input);
    let placeholder = first_placeholder(&parsed);

    assert_eq!(placeholder.formatter_name, "default");
    let nested = placeholder.format.as_ref().unwrap();
    assert_eq!(&input[nested.start..nested.end], "N2");
}

#[test]
fn alignment_before_a_standard_format_specifier() {
    let parsed = parse("{0,10:N2}");
    let placeholder = first_placeholder(&parsed);

    assert_eq!(placeholder.alignment, 10);
    assert_eq!(placeholder.formatter_name, "");
    assert_eq!(placeholder.format.as_ref().unwrap().to_string(), "N2");
}

#[test]
fn real_world_templates() {
    // The example from the .NET Placeholder documentation.
    let input = "{Items.Length,-10:choose(1|2|3):one|two|three}";
    let parsed = parse(input);
    let placeholder = first_placeholder(&parsed);

    assert_eq!(placeholder.alignment, -10);
    assert_eq!(placeholder.formatter_name, "choose");
    assert_eq!(placeholder.formatter_options, "1|2|3");
    assert_eq!(
        placeholder.format.as_ref().unwrap().to_string(),
        "one|two|three"
    );
    assert_eq!(placeholder.to_string(), input);

    // A list format with nested placeholders in the format.
    let input = "{Items:list:{Name}|, |, and }";
    let parsed = parse(input);
    let placeholder = first_placeholder(&parsed);

    assert_eq!(placeholder.formatter_name, "list");
    let nested = placeholder.format.as_ref().unwrap();
    assert!(nested.has_nested());
    assert_eq!(nested.to_string(), "{Name}|, |, and ");
    assert_eq!(parsed.to_string(), input);

    // Conditional formatting keeps the pipes as literal text.
    let input = "{Count:cond:{} item|{} items}";
    let parsed = parse(input);
    let placeholder = first_placeholder(&parsed);
    assert_eq!(placeholder.formatter_name, "cond");
    assert_eq!(
        placeholder.format.as_ref().unwrap().items.len(),
        4,
        "two placeholders, each followed by literal text"
    );
    assert_eq!(parsed.to_string(), input);
}

#[test]
fn too_many_closing_braces_stay_in_the_output() {
    let parser = parser_with(ParserSettings {
        error_action: ErrorAction::Ignore,
        ..ParserSettings::default()
    });
    let parsed = parser.parse("0}").unwrap();

    assert_eq!(parsed.items.len(), 2);
    assert_eq!(parsed.to_string(), "0}");
}
