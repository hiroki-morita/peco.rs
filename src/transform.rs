use std::fmt::Display;

use crate::{Diagnostic, Parser, Span, error::Failure};

impl<T: 'static> Parser<T> {
    /// Converts successful output into an owned value.
    ///
    /// Use pure transformations: lookahead and failing alternatives may execute
    /// this function too. External side effects are never rolled back.
    ///
    /// ```
    /// use peg_combinator::char;
    /// assert_eq!(char('x').map(|c| c.to_string()).parse("x"), Ok("x".to_owned()));
    /// ```
    pub fn map<U: 'static>(self, f: impl Fn(T) -> U + 'static) -> Parser<U> {
        self.wrap(move |parser, ctx, pos| {
            parser.run(ctx, pos).map(|(value, next)| (f(value), next))
        })
    }

    /// Converts output, treating conversion failure as a recoverable mismatch.
    /// The diagnostic includes the consumed span and its ending byte offset.
    /// External side effects are never rolled back, even in discarded branches.
    pub fn try_map<U: 'static, E: Display>(
        self,
        f: impl Fn(T) -> Result<U, E> + 'static,
    ) -> Parser<U> {
        self.wrap(move |parser, ctx, pos| {
            let (value, next) = parser.run(ctx, pos)?;
            match f(value) {
                Ok(value) => Ok((value, next)),
                Err(error) => {
                    let mut diagnostic = Diagnostic::expected(next, "valid converted value");
                    diagnostic.message = Some(error.to_string());
                    diagnostic.span = Some(Span {
                        start: pos,
                        end: next,
                    });
                    Err(Failure::Mismatch(diagnostic))
                }
            }
        })
    }

    /// Discards output and copies the exact matched input into a string.
    pub fn text(self) -> Parser<String> {
        self.wrap(|parser, ctx, pos| {
            let (_, next) = parser.run(ctx, pos)?;
            Ok((ctx.input[pos..next].to_owned(), next))
        })
    }

    /// Returns the parsed value together with its half-open byte range.
    pub fn spanned(self) -> Parser<(T, Span)> {
        self.wrap(|parser, ctx, pos| {
            let (value, next) = parser.run(ctx, pos)?;
            Ok((
                (
                    value,
                    Span {
                        start: pos,
                        end: next,
                    },
                ),
                next,
            ))
        })
    }

    /// Discards the parsed value.
    pub fn ignore(self) -> Parser<()> {
        self.map(|_| ())
    }

    /// Runs both parsers, retaining only this parser's value.
    pub fn then_ignore<U: 'static>(self, other: Parser<U>) -> Self {
        self.then(other).map(|(value, _)| value)
    }

    /// Runs both parsers, retaining only the second parser's value.
    pub fn ignore_then<U: 'static>(self, other: Parser<U>) -> Parser<U> {
        self.then(other).map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, char, literal, satisfy};

    #[derive(Debug, PartialEq)]
    struct Assignment {
        name: String,
        value: u8,
    }

    #[test]
    fn builds_struct_and_enum_outputs() {
        let name = satisfy("letter", |c| c.is_ascii_alphabetic()).plus().text();
        let number = satisfy("digit", |c| c.is_ascii_digit())
            .plus()
            .text()
            .try_map(|s| s.parse::<u8>());
        let assignment = name
            .then_ignore(literal("="))
            .then(number)
            .map(|(name, value)| Assignment { name, value });
        assert_eq!(
            assignment.parse("count=42"),
            Ok(Assignment {
                name: "count".into(),
                value: 42
            })
        );
        #[derive(Debug, PartialEq)]
        enum Value {
            A,
            B,
        }
        let parser = char('a').map(|_| Value::A).or(char('b').map(|_| Value::B));
        assert_eq!(parser.parse("b"), Ok(Value::B));
    }

    #[test]
    fn conversions_backtrack_and_report_ranges() {
        let number = satisfy("digit", |c| c.is_ascii_digit())
            .plus()
            .text()
            .try_map(|s| s.parse::<u8>());
        let ParseError::Mismatch(d) = literal("猫")
            .ignore_then(number.clone())
            .parse("猫256")
            .unwrap_err()
        else {
            panic!("expected mismatch")
        };
        assert_eq!(d.offset, 6);
        assert_eq!(d.span, Some(Span { start: 3, end: 6 }));
        assert!(d.message.is_some());
        assert_eq!(number.or(literal("256").map(|_| 0)).parse("256"), Ok(0));
    }

    #[test]
    fn text_and_span_preserve_unicode_ranges_and_original_output() {
        let parser = literal("a")
            .ignore_then(char('猫').spanned())
            .then_ignore(literal("!"));
        assert_eq!(parser.parse("a猫!"), Ok(('猫', Span { start: 1, end: 4 })));
        assert_eq!(
            char('猫').then(char('犬')).text().parse("猫犬"),
            Ok("猫犬".into())
        );
        assert_eq!(literal("").text().parse(""), Ok(String::new()));
        assert_eq!(char('猫').ignore().parse("猫"), Ok(()));
    }

    #[test]
    fn failed_branch_drops_values_and_side_effects_are_not_rolled_back() {
        use std::{cell::Cell, rc::Rc};
        struct Tracked(Rc<Cell<usize>>);
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let drops = Rc::new(Cell::new(0));
        let seen = drops.clone();
        let parser = literal("a")
            .map(move |_| Tracked(seen.clone()))
            .then_ignore(literal("b"));
        assert!(parser.parse("ax").is_err());
        assert_eq!(drops.get(), 1);
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let parser = literal("a").map(move |_| seen.set(seen.get() + 1));
        assert_eq!(
            crate::and(parser.clone()).ignore_then(parser).parse("a"),
            Ok(())
        );
        assert_eq!(count.get(), 2);
    }
}
