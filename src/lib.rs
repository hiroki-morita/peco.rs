//! PEG parser combinators with owned outputs and explicit backtracking.
//!
//! Parsers share their executable through `Rc` and are intended for single-threaded use.
//! Input positions are UTF-8 byte offsets. Parsing does not skip whitespace implicitly.

mod error;
mod parser;
mod primitive;

pub use error::{Diagnostic, GrammarError, ParseError};
pub use parser::{ParseOutput, Parser, Span};

pub use primitive::{any_char, char, char_range, end, literal, one_of, satisfy};
