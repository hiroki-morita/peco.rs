use crate::{Diagnostic, GrammarError, Parser, error::Failure};

impl<T: 'static> Parser<T> {
    /// Runs both parsers in order, returning both values.
    pub fn then<U: 'static>(self, other: Parser<U>) -> Parser<(T, U)> {
        self.combine(other, |left, right, ctx, pos| {
            let (a, next) = left.run(ctx, pos)?;
            let (b, end) = right.run(ctx, next)?;
            Ok(((a, b), end))
        })
    }

    /// Tries the right parser at the original position only if the left mismatches.
    /// A successful left alternative is never revisited by a later parser.
    pub fn or(self, other: Self) -> Self {
        self.combine(other, |left, right, ctx, pos| match left.run(ctx, pos) {
            Err(Failure::Mismatch(a)) => match right.run(ctx, pos) {
                Err(Failure::Mismatch(b)) => Err(Failure::Mismatch(a.merge(b))),
                result => result,
            },
            result => result,
        })
    }

    /// Greedily matches zero or more occurrences without giving input back.
    /// Empty successes are fatal grammar errors.
    pub fn star(self) -> Parser<Vec<T>> {
        self.repeat(false)
    }

    /// Greedily matches one or more occurrences without giving input back.
    /// Empty successes are fatal grammar errors.
    pub fn plus(self) -> Parser<Vec<T>> {
        self.repeat(true)
    }

    fn repeat(self, require_one: bool) -> Parser<Vec<T>> {
        self.wrap(move |parser, ctx, mut pos| {
            let mut values = Vec::new();
            loop {
                match parser.run(ctx, pos) {
                    Ok((value, next)) => {
                        if next == pos {
                            return Err(Failure::Fatal(GrammarError::NonProgressingRepetition {
                                offset: pos,
                            }));
                        }
                        values.push(value);
                        pos = next;
                    }
                    Err(Failure::Mismatch(d)) if require_one && values.is_empty() => {
                        return Err(Failure::Mismatch(d));
                    }
                    Err(Failure::Mismatch(_)) => return Ok((values, pos)),
                    Err(error) => return Err(error),
                }
            }
        })
    }

    /// Converts a mismatch into `None` without consuming input.
    pub fn optional(self) -> Parser<Option<T>> {
        self.wrap(|parser, ctx, pos| match parser.run(ctx, pos) {
            Ok((value, next)) => Ok((Some(value), next)),
            Err(Failure::Mismatch(_)) => Ok((None, pos)),
            Err(error) => Err(error),
        })
    }
}

/// Positive lookahead: succeeds without consuming input when `parser` succeeds.
/// Parsed values are discarded; semantic actions may still run.
pub fn and<T: 'static>(parser: Parser<T>) -> Parser<()> {
    parser.wrap(|parser, ctx, pos| parser.run(ctx, pos).map(|_| ((), pos)))
}

/// Negative lookahead: succeeds without consuming input when `parser` mismatches.
/// Fatal errors are propagated, not inverted. Semantic actions may still run.
pub fn not<T: 'static>(parser: Parser<T>) -> Parser<()> {
    parser.wrap(|parser, ctx, pos| match parser.run(ctx, pos) {
        Ok(_) => Err(Failure::Mismatch(Diagnostic::expected(
            pos,
            "expression not to match",
        ))),
        Err(Failure::Mismatch(_)) => Ok(((), pos)),
        Err(error) => Err(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, literal};

    fn diagnostic<T>(result: Result<T, ParseError>) -> Diagnostic {
        match result {
            Err(ParseError::Mismatch(d)) => d,
            _ => panic!("expected mismatch"),
        }
    }

    #[test]
    fn ordered_choice_rolls_back_only_on_failure() {
        assert_eq!(literal("ab").or(literal("ac")).parse("ac"), Ok(()));
        assert!(
            literal("a")
                .or(literal("ab"))
                .then(literal("c"))
                .parse("abc")
                .is_err()
        );
        assert_eq!(
            literal("ab").optional().then(literal("ac")).parse("ac"),
            Ok((None, ()))
        );
    }

    #[test]
    fn repetition_is_greedy_and_requires_progress() {
        assert!(literal("a").star().then(literal("a")).parse("aaa").is_err());
        assert_eq!(literal("a").star().parse(""), Ok(vec![]));
        assert_eq!(literal("a").plus().parse("aa"), Ok(vec![(), ()]));
        assert!(literal("a").plus().parse("").is_err());
        assert!(matches!(
            literal("").star().parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(
            literal("").plus().parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(
            literal("a").optional().star().parse(""),
            Err(ParseError::Fatal(_))
        ));
    }

    #[test]
    fn lookahead_restores_position_and_discards_stop_diagnostics() {
        assert_eq!(
            and(literal("猫")).then(literal("猫")).parse("猫"),
            Ok(((), ()))
        );
        assert_eq!(
            not(literal("ab")).then(literal("ac")).parse("ac"),
            Ok(((), ()))
        );
        assert_eq!(diagnostic(not(literal("a")).parse("a")).offset, 0);
        assert_eq!(diagnostic(and(literal("ab")).parse("ax")).offset, 1);
        for d in [
            diagnostic(literal("ab").star().then(literal("z")).parse("ax")),
            diagnostic(literal("ab").optional().then(literal("z")).parse("ax")),
        ] {
            assert_eq!(d.offset, 0);
            assert_eq!(d.expected, ["\"z\"".to_owned()].into());
        }
        let d = diagnostic(literal("ab").or(literal("ac")).parse("ax"));
        assert_eq!(d.offset, 1);
        assert_eq!(d.expected.len(), 2);
        assert_eq!(
            diagnostic(literal("ab").or(literal("z")).parse("ax")).offset,
            1
        );
        assert_eq!(
            diagnostic(literal("a").or(literal("a")).parse("x"))
                .expected
                .len(),
            1
        );
    }

    #[test]
    fn fatal_errors_survive_every_control_combinator() {
        let bad = literal("").star();
        assert!(matches!(
            bad.clone().or(literal("a").star()).parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(
            bad.clone().optional().parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(
            bad.clone().star().parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(
            bad.clone().plus().parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(
            and(bad.clone()).parse(""),
            Err(ParseError::Fatal(_))
        ));
        assert!(matches!(not(bad).parse(""), Err(ParseError::Fatal(_))));
    }
}
