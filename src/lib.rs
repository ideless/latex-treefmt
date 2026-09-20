//! A syntax-tree-based LaTeX formatter.
//!
//! `tree-sitter-latex` constructs the concrete syntax tree. The writer remains
//! source-backed: it walks every child, including anonymous tokens, and copies
//! the gaps between nodes. This guarantees that unsupported syntax is retained.

#![forbid(unsafe_code)]

mod parser;
mod writer;

pub use parser::{LatexParser, ParseError};
pub use writer::{
    DisplayMathDelimiters, DisplayMathLayout, InlineMathDelimiters, LatexWriter, LineEnding,
    WriteError, WriterOptions,
};

use std::error::Error;
use std::fmt;

/// The result of one formatting pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOutput {
    /// The rendered LaTeX document.
    pub text: String,
    /// Whether Tree-sitter recovered from malformed or unsupported input.
    pub had_parse_errors: bool,
}

/// An error raised while parsing or writing a document.
#[derive(Debug)]
pub enum FormatError {
    Parse(ParseError),
    Write(WriteError),
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "failed to parse LaTeX: {error}"),
            Self::Write(error) => write!(formatter, "failed to write LaTeX: {error}"),
        }
    }
}

impl Error for FormatError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::Write(error) => Some(error),
        }
    }
}

impl From<ParseError> for FormatError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

impl From<WriteError> for FormatError {
    fn from(error: WriteError) -> Self {
        Self::Write(error)
    }
}

/// Parse and format one LaTeX document.
pub fn format(source: &str, options: &WriterOptions) -> Result<FormatOutput, FormatError> {
    let mut parser = LatexParser::new()?;
    let tree = parser.parse(source)?;
    let had_parse_errors = tree.root_node().has_error();
    let text = LatexWriter::new(source, options).write(&tree)?;

    Ok(FormatOutput {
        text,
        had_parse_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::{WriterOptions, format};

    #[test]
    fn recovered_input_is_reported_and_remains_lossless() {
        let source = "$ foo $$ bar";
        let output = format(source, &WriterOptions::preserve()).unwrap();

        assert_eq!(output.text, source);
        assert!(output.had_parse_errors);
    }
}
