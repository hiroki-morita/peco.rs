use std::ops::RangeInclusive;

use crate::{Diagnostic, Parser, error::Failure};

/// Matches an exact string, including the empty string, and discards its value.
///
/// ```
/// use peco::literal;
/// assert_eq!(literal("猫").parse("猫"), Ok(()));
/// ```
pub fn literal(text: impl Into<String>) -> Parser<()> {
    let text = text.into();
    Parser::new(move |ctx, pos| {
        let mut offset = pos;
        for expected in text.chars() {
            match ctx.input[offset..].chars().next() {
                Some(actual) if actual == expected => offset += actual.len_utf8(),
                _ => {
                    return Err(Failure::Mismatch(Diagnostic::expected(
                        offset,
                        format!("{text:?}"),
                    )));
                }
            }
        }
        Ok(((), offset))
    })
}

/// Matches one Unicode scalar value accepted by the predicate.
/// `name` describes the expected character class in diagnostics.
pub fn satisfy(
    name: impl Into<String>,
    predicate: impl Fn(char) -> bool + 'static,
) -> Parser<char> {
    let name = name.into();
    Parser::new(move |ctx, pos| match ctx.input[pos..].chars().next() {
        Some(c) if predicate(c) => Ok((c, pos + c.len_utf8())),
        _ => Err(Failure::Mismatch(Diagnostic::expected(pos, name.clone()))),
    })
}

/// Matches one Unicode scalar value, including a newline; fails at EOF.
pub fn any_char() -> Parser<char> {
    satisfy("any character", |_| true)
}

/// Matches exactly one specified Unicode scalar value.
pub fn char(expected: char) -> Parser<char> {
    satisfy(format!("{expected:?}"), move |c| c == expected)
}

/// Matches one scalar value in an inclusive range. Reversed ranges match nothing.
pub fn char_range(range: RangeInclusive<char>) -> Parser<char> {
    satisfy(format!("{range:?}"), move |c| range.contains(&c))
}

/// Matches one scalar value from a set represented as a string.
/// An empty set matches nothing; duplicates have no effect.
pub fn one_of(characters: impl Into<String>) -> Parser<char> {
    let characters = characters.into();
    satisfy(format!("one of {characters:?}"), move |c| {
        characters.contains(c)
    })
}

/// Succeeds without consuming input only at the end of input.
pub fn end() -> Parser<()> {
    Parser::new(|ctx, pos| {
        if pos == ctx.input.len() {
            Ok(((), pos))
        } else {
            Err(Failure::Mismatch(Diagnostic::expected(pos, "end of input")))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseError;

    #[test]
    fn literals_match_empty_unicode_and_newlines() {
        assert_eq!(literal("").parse(""), Ok(()));
        assert_eq!(literal("").parse_prefix("猫").unwrap().consumed, 0);
        assert_eq!(literal("猫\n").parse("猫\n"), Ok(()));
        assert_eq!(
            literal("ab").parse("a!"),
            Err(ParseError::Mismatch(Diagnostic::expected(1, "\"ab\"")))
        );
        assert_eq!(
            literal("猫犬").parse("猫"),
            Err(ParseError::Mismatch(Diagnostic::expected(3, "\"猫犬\"")))
        );
    }

    #[test]
    fn scalar_parsers_handle_eof_and_byte_lengths() {
        for parser in [
            any_char(),
            char('猫'),
            satisfy("cat", |c| c == '猫'),
            one_of("猫犬"),
        ] {
            assert_eq!(parser.parse_prefix("猫x").unwrap().consumed, 3);
            assert_eq!(parser.parse("猫"), Ok('猫'));
            assert!(matches!(parser.parse(""), Err(ParseError::Mismatch(_))));
        }
        assert_eq!(any_char().parse("\n"), Ok('\n'));
        assert!(char('x').parse("y").is_err());
        assert!(
            satisfy("ASCII digit", |c| c.is_ascii_digit())
                .parse("１")
                .is_err()
        );
    }

    #[test]
    fn ranges_and_sets_include_boundaries() {
        let range = char_range('a'..='z');
        assert_eq!(range.parse("a"), Ok('a'));
        assert_eq!(range.parse("z"), Ok('z'));
        assert!(range.parse("A").is_err());
        assert!(
            char_range(RangeInclusive::new('z', 'a'))
                .parse("m")
                .is_err()
        );
        assert!(one_of("").parse("a").is_err());
        assert_eq!(one_of("aa").parse("a"), Ok('a'));
    }

    #[test]
    fn eof_does_not_consume_or_accept_a_suffix() {
        assert_eq!(end().parse(""), Ok(()));
        assert!(end().parse_prefix("a").is_err());
    }
}
