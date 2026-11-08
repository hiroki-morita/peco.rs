use std::{collections::BTreeSet, error::Error, fmt};

use crate::Span;

/// An input mismatch, including the expected items at a byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// UTF-8 byte offset of the mismatch.
    pub offset: usize,
    /// Expected items in deterministic order.
    pub expected: BTreeSet<String>,
    /// Names of enclosing grammar rules.
    pub context: Vec<String>,
    /// Optional explanation from a result conversion.
    pub message: Option<String>,
    /// Optional input range associated with a result conversion.
    pub span: Option<Span>,
}

impl Diagnostic {
    // Keeps the farthest mismatch, unioning expectations at equal offsets.
    pub(crate) fn merge(mut self, other: Self) -> Self {
        match self.offset.cmp(&other.offset) {
            std::cmp::Ordering::Less => other,
            std::cmp::Ordering::Greater => self,
            std::cmp::Ordering::Equal => {
                self.expected.extend(other.expected);
                // Keep the first branch's explanation and rule path at ties.
                self
            }
        }
    }

    /// Creates a mismatch with one expected item.
    pub fn expected(offset: usize, expected: impl Into<String>) -> Self {
        Self {
            offset,
            expected: BTreeSet::from([expected.into()]),
            context: Vec::new(),
            message: None,
            span: None,
        }
    }
}

/// A grammar or execution error that must never be treated as a mismatch.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrammarError {
    /// A repeated parser succeeded without consuming input.
    NonProgressingRepetition { offset: usize },
    /// A rule was re-entered at the same input position before returning.
    LeftRecursion { rule: String, offset: usize },
    /// The configured recursion limit was reached.
    RecursionLimit { limit: usize, offset: usize },
    /// A rule has no definition yet.
    UndefinedRule { rule: String },
    /// A rule was defined more than once.
    DuplicateDefinition { rule: String },
    /// A rule belongs to another grammar builder.
    ForeignRule { rule: String },
    /// A reference outlived the grammar that owns its definition.
    ExpiredRule { rule: String },
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonProgressingRepetition { offset } => {
                write!(f, "repetition did not consume input at byte {offset}")
            }
            Self::LeftRecursion { rule, offset } => {
                write!(f, "left recursion in {rule} at byte {offset}")
            }
            Self::RecursionLimit { limit, offset } => {
                write!(f, "recursion limit {limit} reached at byte {offset}")
            }
            Self::UndefinedRule { rule } => write!(f, "undefined rule: {rule}"),
            Self::DuplicateDefinition { rule } => write!(f, "duplicate definition: {rule}"),
            Self::ForeignRule { rule } => write!(f, "rule belongs to another grammar: {rule}"),
            Self::ExpiredRule { rule } => write!(f, "grammar no longer owns rule: {rule}"),
        }
    }
}

impl Error for GrammarError {}

/// Distinguishes recoverable input mismatches from fatal grammar errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// A parser did not match the input.
    Mismatch(Diagnostic),
    /// Invalid grammar or an execution limit; alternatives must not hide it.
    Fatal(GrammarError),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch(diagnostic) => {
                write!(f, "expected ")?;
                for (index, item) in diagnostic.expected.iter().enumerate() {
                    if index != 0 {
                        write!(f, " or ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, " at byte {}", diagnostic.offset)?;
                if let Some(message) = &diagnostic.message {
                    write!(f, ": {message}")?;
                }
                if let Some(span) = diagnostic.span {
                    write!(f, " (bytes {}..{})", span.start, span.end)?;
                }
                if !diagnostic.context.is_empty() {
                    write!(f, " [in {}]", diagnostic.context.join(" > "))?;
                }
                Ok(())
            }
            Self::Fatal(error) => error.fmt(f),
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Fatal(error) => Some(error),
            Self::Mismatch(_) => None,
        }
    }
}

// Uses the public error representation for internal recoverable and fatal failures.
pub(crate) use ParseError as Failure;

/// An error displayed with a caller-supplied input for line and column lookup.
/// Supply the original input that produced the error.
pub struct InputDiagnostic<'a> {
    // Borrows the diagnostic without copying its messages or rule context.
    error: &'a ParseError,
    // Borrows the original input for on-demand line and column calculation.
    input: &'a str,
}

impl ParseError {
    /// Adds one-based line and Unicode scalar column numbers to the display.
    /// Newlines are LF; tabs count as one scalar. Invalid offsets are reported
    /// explicitly instead of slicing at a non-character boundary.
    pub fn display_with_input<'a>(&'a self, input: &'a str) -> InputDiagnostic<'a> {
        InputDiagnostic { error: self, input }
    }
}

impl fmt::Display for InputDiagnostic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;
        let offset = match self.error {
            ParseError::Mismatch(d) => Some(d.offset),
            ParseError::Fatal(
                GrammarError::NonProgressingRepetition { offset }
                | GrammarError::LeftRecursion { offset, .. }
                | GrammarError::RecursionLimit { offset, .. },
            ) => Some(*offset),
            _ => None,
        };
        if let Some(offset) = offset {
            if let Some(prefix) = self.input.get(..offset) {
                let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
                write!(f, " (line {line}, column {column})")?;
            } else {
                write!(f, " (invalid input offset {offset})")?;
            }
        }
        Ok(())
    }
}

impl<T: 'static> crate::Parser<T> {
    /// Replaces mismatch expectations with a user-facing label.
    /// Keeps the failure position, conversion explanation, and rule context.
    pub fn label(self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.wrap(move |parser, ctx, pos| {
            parser.run(ctx, pos).map_err(|error| match error {
                Failure::Mismatch(mut d) => {
                    d.expected = BTreeSet::from([name.clone()]);
                    Failure::Mismatch(d)
                }
                error => error,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{and, literal, not, recursive};

    #[test]
    fn labels_and_rule_context_survive_conversion_errors() {
        let number = recursive("Number", |_| {
            literal("256")
                .text()
                .try_map(|s| s.parse::<u8>())
                .label("small integer")
        });
        let root = recursive("Root", |_| literal("猫\n").ignore_then(number));
        let error = root.parse("猫\n256").unwrap_err();
        let ParseError::Mismatch(d) = &error else {
            panic!("expected mismatch")
        };
        assert_eq!(d.context, ["Root", "Number"]);
        assert_eq!(d.expected, ["small integer".to_owned()].into());
        assert_eq!(d.span, Some(Span { start: 4, end: 7 }));
        let text = error.display_with_input("猫\n256").to_string();
        assert!(text.contains("line 2, column 4"));
        assert!(text.contains("bytes 4..7"));
        assert!(text.contains("Root > Number"));
        assert!(text.contains(d.message.as_ref().unwrap()));
    }

    #[test]
    fn unicode_columns_eof_and_invalid_offsets_are_safe() {
        for (input, offset, location) in [
            ("猫\n犬", 7, "line 2, column 2"),
            ("猫\t", 4, "line 1, column 3"),
            ("", 0, "line 1, column 1"),
        ] {
            let error = ParseError::Mismatch(Diagnostic::expected(offset, "x"));
            assert!(
                error
                    .display_with_input(input)
                    .to_string()
                    .contains(location)
            );
        }
        for offset in [1, 99] {
            let error = ParseError::Mismatch(Diagnostic::expected(offset, "x"));
            assert!(
                error
                    .display_with_input("猫")
                    .to_string()
                    .contains("invalid input offset")
            );
        }
        let error = ParseError::Fatal(GrammarError::RecursionLimit {
            limit: 0,
            offset: 0,
        });
        assert!(error.source().is_some());
        assert!(
            error
                .display_with_input("")
                .to_string()
                .contains("line 1, column 1")
        );
    }

    #[test]
    fn lookahead_and_successful_branches_do_not_pollute_diagnostics() {
        let parser = literal("abc").or(literal("a")).then(literal("!"));
        let ParseError::Mismatch(d) = parser.parse("abx").unwrap_err() else {
            panic!()
        };
        assert_eq!(d.offset, 1);
        assert_eq!(d.expected, ["\"!\"".to_owned()].into());
        let ParseError::Mismatch(d) = not(literal("abc"))
            .then(literal("!"))
            .parse("abx")
            .unwrap_err()
        else {
            panic!()
        };
        assert_eq!(d.offset, 0);
        assert_eq!(d.expected, ["\"!\"".to_owned()].into());
        let parser = and(literal("abc").label("word"));
        assert!(
            parser
                .parse("abx")
                .unwrap_err()
                .to_string()
                .contains("word at byte 2")
        );
        let error = literal("b")
            .or(literal("a"))
            .or(literal("b"))
            .parse("x")
            .unwrap_err();
        assert_eq!(error.to_string(), "expected \"a\" or \"b\" at byte 0");
        assert!(matches!(
            literal("").star().label("list").parse(""),
            Err(ParseError::Fatal(_))
        ));
    }
}
