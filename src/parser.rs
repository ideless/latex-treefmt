use std::error::Error;
use std::fmt;

use tree_sitter::{LanguageError, Parser, Tree};

/// A reusable `tree-sitter-latex` parser.
pub struct LatexParser {
    parser: Parser,
}

impl LatexParser {
    /// Construct a parser configured with the pinned LaTeX grammar.
    pub fn new() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_latex::language())
            .map_err(ParseError::Language)?;
        Ok(Self { parser })
    }

    /// Parse a UTF-8 LaTeX source document.
    pub fn parse(&mut self, source: &str) -> Result<Tree, ParseError> {
        self.parser.parse(source, None).ok_or(ParseError::Cancelled)
    }
}

/// An error raised before Tree-sitter can return a syntax tree.
#[derive(Debug)]
pub enum ParseError {
    Language(LanguageError),
    Cancelled,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Language(error) => write!(formatter, "could not load the LaTeX grammar: {error}"),
            Self::Cancelled => formatter.write_str("the parser was cancelled"),
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Language(error) => Some(error),
            Self::Cancelled => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LatexParser;

    #[test]
    fn constructs_a_tree_for_math() {
        let mut parser = LatexParser::new().unwrap();
        let tree = parser.parse(r"$\sqrt[3]{x}$").unwrap();

        assert!(!tree.root_node().has_error());
        assert_eq!(tree.root_node().kind(), "source_file");
    }
}
