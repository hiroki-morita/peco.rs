use std::rc::Rc;

use crate::{Diagnostic, ParseError, error::Failure};

/// A half-open input range in UTF-8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Inclusive start offset.
    pub start: usize,
    /// Exclusive end offset.
    pub end: usize,
}

/// A prefix parse result and the number of input bytes consumed.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseOutput<T> {
    /// The parsed value.
    pub value: T,
    /// UTF-8 byte offset immediately after the matched prefix.
    pub consumed: usize,
}

pub(crate) struct Context<'i> {
    pub(crate) input: &'i str,
}

pub(crate) type Step<T> = Result<(T, usize), Failure>;
type Exec<T> = dyn for<'i> Fn(&mut Context<'i>, usize) -> Step<T>;

/// A reusable, shared parser producing an owned value.
///
/// Cloning shares the executable and never requires `T: Clone`. Each parse has
/// its own execution context; the input need not have a `'static` lifetime.
/// This type does not implement `Send` or `Sync`.
pub struct Parser<T> {
    exec: Rc<Exec<T>>,
}

impl<T> Clone for Parser<T> {
    fn clone(&self) -> Self {
        Self {
            exec: self.exec.clone(),
        }
    }
}

impl<T: 'static> Parser<T> {
    pub(crate) fn new(exec: impl for<'i> Fn(&mut Context<'i>, usize) -> Step<T> + 'static) -> Self {
        Self {
            exec: Rc::new(exec),
        }
    }

    /// Parses the entire input, returning a mismatch if a suffix remains.
    pub fn parse(&self, input: &str) -> Result<T, ParseError> {
        let output = self.parse_prefix(input)?;
        if output.consumed == input.len() {
            Ok(output.value)
        } else {
            Err(Failure::Mismatch(Diagnostic::expected(
                output.consumed,
                "end of input",
            )))
        }
    }

    /// Parses a prefix without requiring the end of input.
    pub fn parse_prefix(&self, input: &str) -> Result<ParseOutput<T>, ParseError> {
        let mut context = Context { input };
        let (value, consumed) = (self.exec)(&mut context, 0)?;
        debug_assert!(context.input.is_char_boundary(consumed));
        Ok(ParseOutput { value, consumed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GrammarError;

    #[test]
    fn prefix_and_full_parse_use_byte_offsets() {
        let parser = Parser::new(|ctx, pos| {
            let Some(c) = ctx.input[pos..].chars().next() else {
                return Err(Failure::Mismatch(Diagnostic::expected(pos, "character")));
            };
            Ok((c, pos + c.len_utf8()))
        });
        let input = String::from("猫a");
        assert_eq!(
            parser.parse_prefix(&input),
            Ok(ParseOutput {
                value: '猫',
                consumed: 3
            })
        );
        assert_eq!(
            parser.parse(&input),
            Err(ParseError::Mismatch(Diagnostic::expected(
                3,
                "end of input"
            )))
        );
        assert_eq!(
            parser.parse(""),
            Err(ParseError::Mismatch(Diagnostic::expected(0, "character")))
        );
        assert_eq!(parser.parse("猫"), Ok('猫'));
    }

    #[test]
    fn clone_and_repeated_execution_do_not_require_clone_output() {
        #[derive(Debug, PartialEq)]
        struct Owned(String);
        let parser = Parser::new(|ctx, _| Ok((Owned(ctx.input.to_owned()), ctx.input.len())));
        let clone = parser.clone();
        drop(parser);
        for input in [String::from("first"), String::from("second"), String::new()] {
            assert_eq!(clone.parse(&input), Ok(Owned(input)));
        }
    }

    #[test]
    fn fatal_errors_remain_distinct_from_mismatches() {
        let parser = Parser::<()>::new(|_, pos| {
            Err(Failure::Fatal(GrammarError::NonProgressingRepetition {
                offset: pos,
            }))
        });
        let expected = Err(ParseError::Fatal(GrammarError::NonProgressingRepetition {
            offset: 0,
        }));
        assert_eq!(parser.parse("abc"), expected);
        assert_eq!(parser.parse_prefix("abc").map(|o| o.value), expected);
    }

    #[test]
    fn empty_success_is_valid_outside_repetition() {
        let parser = Parser::new(|_, pos| Ok(((), pos)));
        assert_eq!(parser.parse(""), Ok(()));
        assert_eq!(
            parser.parse_prefix("abc"),
            Ok(ParseOutput {
                value: (),
                consumed: 0
            })
        );
    }
}
