use std::{collections::HashSet, rc::Rc};

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

/// Execution settings applied independently to each parse call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Maximum simultaneously active nonterminal calls. Zero rejects every call.
    /// Raising this limit may exhaust the thread stack; it is not a stack size guarantee.
    pub recursion_limit: usize,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            recursion_limit: 64,
        }
    }
}

// Holds input and execution state for one independent parse call.
pub(crate) struct Context<'i> {
    // Borrows the original input without extending its lifetime.
    pub(crate) input: &'i str,
    // Tracks active (rule identity, byte offset) pairs for recursion checks.
    pub(crate) active: HashSet<(usize, usize)>,
    // Keeps execution limits local to this parse call.
    pub(crate) options: ParseOptions,
}

// Returns a value and next byte offset, or an unconsumed failure.
pub(crate) type Step<T> = Result<(T, usize), Failure>;
// Erases the executable type while accepting input of any lifetime.
type Exec<T> = dyn for<'i> Fn(&mut Context<'i>, usize) -> Step<T>;

/// A reusable, shared parser producing an owned value.
///
/// Cloning shares the executable and never requires `T: Clone`. Each parse has
/// its own execution context; the input need not have a `'static` lifetime.
/// This type does not implement `Send` or `Sync`.
pub struct Parser<T> {
    // Shares the executable without requiring the output to be Clone.
    exec: Rc<Exec<T>>,
    // Shares unresolved rule references for grammar validation after composition.
    pub(crate) dependencies: Rc<Vec<crate::rule::Dependency>>,
}

impl<T> Clone for Parser<T> {
    fn clone(&self) -> Self {
        Self {
            exec: self.exec.clone(),
            dependencies: self.dependencies.clone(),
        }
    }
}

impl<T: 'static> Parser<T> {
    // Creates an executable with no unresolved rule references.
    pub(crate) fn new(exec: impl for<'i> Fn(&mut Context<'i>, usize) -> Step<T> + 'static) -> Self {
        Self {
            exec: Rc::new(exec),
            dependencies: Rc::new(Vec::new()),
        }
    }

    // Executes at a caller-supplied byte offset in the current context.
    pub(crate) fn run(&self, ctx: &mut Context<'_>, pos: usize) -> Step<T> {
        (self.exec)(ctx, pos)
    }

    // Wraps one parser while preserving its unresolved rule references.
    pub(crate) fn wrap<U: 'static>(
        self,
        f: impl Fn(&Self, &mut Context<'_>, usize) -> Step<U> + 'static,
    ) -> Parser<U> {
        let dependencies = self.dependencies.clone();
        let mut result = Parser::new(move |ctx, pos| f(&self, ctx, pos));
        result.dependencies = dependencies;
        result
    }

    // Combines two parsers and carries both sets of unresolved references.
    pub(crate) fn combine<U: 'static, V: 'static>(
        self,
        other: Parser<U>,
        f: impl Fn(&Self, &Parser<U>, &mut Context<'_>, usize) -> Step<V> + 'static,
    ) -> Parser<V> {
        let mut dependencies = self.dependencies.clone();
        Rc::make_mut(&mut dependencies).extend(other.dependencies.iter().cloned());
        let mut result = Parser::new(move |ctx, pos| f(&self, &other, ctx, pos));
        result.dependencies = dependencies;
        result
    }

    /// Parses the entire input, returning a mismatch if a suffix remains.
    pub fn parse(&self, input: &str) -> Result<T, ParseError> {
        self.parse_with_options(input, ParseOptions::default())
    }

    /// Parses the entire input with explicit execution settings.
    pub fn parse_with_options(&self, input: &str, options: ParseOptions) -> Result<T, ParseError> {
        let output = self.parse_prefix_with_options(input, options)?;
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
        self.parse_prefix_with_options(input, ParseOptions::default())
    }

    /// Parses a prefix with explicit execution settings.
    pub fn parse_prefix_with_options(
        &self,
        input: &str,
        options: ParseOptions,
    ) -> Result<ParseOutput<T>, ParseError> {
        let mut context = Context {
            input,
            active: HashSet::new(),
            options,
        };
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
        // Exercises parser sharing with an output that deliberately lacks Clone.
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
