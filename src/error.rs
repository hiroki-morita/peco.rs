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

pub(crate) use ParseError as Failure;
