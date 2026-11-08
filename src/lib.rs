//! peco.rs: PEG parser combinators with owned outputs and explicit backtracking.
//!
//! Parsers share their executable through `Rc` and are intended for single-threaded use.
//! Input positions are UTF-8 byte offsets. Parsing does not skip whitespace implicitly.
//! Outputs own their data (or use [`Span`]); input-borrowing outputs are not supported.
//! Left recursion and empty repetition are errors. There is no packrat memoization
//! or linear-time guarantee. Recursive grammars have a configurable depth limit.

//!
//! Examples from the architecture:
//! ```rust
//! # use peco::*;
//! let digit = satisfy("ASCII digit", |c| c.is_ascii_digit());
//! let digits = digit.clone().plus();
//! let signed_digits = char('-').optional().then(digits);
//! assert_eq!(signed_digits.parse("-42")?.0, Some('-'));
//! # Ok::<(), ParseError>(())
//! ```
//!
//! ```rust
//! # use peco::*;
//! #[derive(Debug, PartialEq)]
//! enum Expr {
//!     Number(u64),
//!     Group(Box<Expr>),
//! }
//!
//! let number = satisfy("ASCII digit", |c| c.is_ascii_digit())
//!     .plus()
//!     .text()
//!     .try_map(|s| s.parse::<u64>())
//!     .map(Expr::Number);
//!
//! let expr = recursive("Expr", |expr| {
//!     let group = literal("(")
//!         .ignore_then(expr)
//!         .then_ignore(literal(")"))
//!         .map(|value| Expr::Group(Box::new(value)));
//!     number.or(group)
//! });
//! let value = expr.parse("((42))")?;
//! assert_eq!(value, Expr::Group(Box::new(Expr::Group(Box::new(Expr::Number(42))))));
//! # Ok::<(), ParseError>(())
//! ```
//!
//! ```rust
//! # use peco::*;
//! #[derive(Debug)]
//! struct Assignment {
//!     name: String,
//!     value: u64,
//! }
//!
//! let name = satisfy("ASCII letter", |c| c.is_ascii_alphabetic())
//!     .plus().text();
//! let value = satisfy("ASCII digit", |c| c.is_ascii_digit())
//!     .plus().text().try_map(|s| s.parse::<u64>());
//! let assignment = name
//!     .then_ignore(literal("="))
//!     .then(value)
//!     .map(|(name, value)| Assignment { name, value });
//! let result = assignment.parse("count=42")?;
//! assert_eq!(result.name, "count");
//! assert_eq!(result.value, 42);
//! # Ok::<(), ParseError>(())
//! ```

mod combinator;
mod error;
mod parser;
mod primitive;
mod rule;
mod transform;

pub use error::{Diagnostic, GrammarError, InputDiagnostic, ParseError};
pub use parser::{ParseOptions, ParseOutput, Parser, Span};

pub use primitive::{any_char, char, char_range, end, literal, one_of, satisfy};

pub use combinator::{and, not};

pub use rule::{GrammarBuilder, Rule, recursive};
