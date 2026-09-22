use std::error::Error;
use std::fmt;
use std::ops::Range;

use tree_sitter::{Node, Tree};

use crate::parser::{LatexParser, ParseError};

/// Line-ending policy for rendered output.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// Keep the ending of each existing source line.
    #[default]
    Preserve,
    /// Render line endings as `\n`.
    Lf,
    /// Render line endings as `\r\n`.
    Crlf,
}

/// Delimiter policy for inline formulas.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum InlineMathDelimiters {
    /// Keep each formula's original delimiters.
    Preserve,
    /// Render inline formulas as `$...$`.
    #[default]
    Dollars,
    /// Render inline formulas as `\(...\)`.
    Parentheses,
}

/// Delimiter policy for displayed formulas.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMathDelimiters {
    /// Keep each formula's original delimiters.
    Preserve,
    /// Render displayed formulas as `$$...$$`.
    #[default]
    Dollars,
    /// Render displayed formulas as `\[...\]`.
    Brackets,
}

/// Line-layout policy for displayed formulas.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMathLayout {
    /// Retain the existing inline or multiline placement exactly.
    Preserve,
    /// Keep single-line formulas inline and normalize multiline formulas as blocks.
    #[default]
    Adaptive,
    /// Render every displayed formula as an indented block.
    Block,
}

/// Configuration for the source-backed LaTeX writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterOptions {
    /// Number of spaces added for each nested environment.
    pub indent_width: usize,
    /// Whether leading indentation should follow environment nesting.
    pub indent_environments: bool,
    /// Whether the contents of the top-level `document` environment are indented.
    pub indent_document: bool,
    /// Remove spaces and tabs immediately before a line ending.
    pub trim_trailing_whitespace: bool,
    /// Maximum consecutive blank lines, or `None` to retain all blank lines.
    pub max_blank_lines: Option<usize>,
    /// Line-ending conversion policy.
    pub line_ending: LineEnding,
    /// Add a final line ending when a non-empty document does not have one.
    pub ensure_final_newline: bool,
    /// Remove unnecessary horizontal whitespace in parsed math regions.
    pub compact_math: bool,
    /// Render single-token scripts without redundant curly braces.
    pub simplify_math_scripts: bool,
    /// Put each prose sentence on its own source line.
    pub sentence_per_line: bool,
    /// Collapse repeated horizontal whitespace in prose to one space.
    pub collapse_prose_whitespace: bool,
    /// Ensure a blank line before and after sectioning commands.
    pub blank_lines_around_sections: bool,
    /// Keep recognized math-environment boundaries on their own lines.
    pub separate_math_environment_boundaries: bool,
    /// Align column separators and row terminators in alignment and table environments.
    pub align_environment_rows: bool,
    /// Ensure every `\item` declaration starts on its own line.
    pub item_starts_on_own_line: bool,
    /// Ensure every item body starts on the line after its declaration.
    pub item_finishes_with_line_break: bool,
    /// Delimiter style used for inline formulas.
    pub inline_math_delimiters: InlineMathDelimiters,
    /// Delimiter style used for displayed formulas.
    pub display_math_delimiters: DisplayMathDelimiters,
    /// Line-layout policy used for displayed formulas.
    pub display_math_layout: DisplayMathLayout,
}

impl Default for WriterOptions {
    fn default() -> Self {
        Self {
            indent_width: 2,
            indent_environments: true,
            indent_document: false,
            trim_trailing_whitespace: true,
            max_blank_lines: Some(1),
            line_ending: LineEnding::Preserve,
            ensure_final_newline: true,
            compact_math: true,
            simplify_math_scripts: true,
            sentence_per_line: true,
            collapse_prose_whitespace: true,
            blank_lines_around_sections: true,
            separate_math_environment_boundaries: true,
            align_environment_rows: true,
            item_starts_on_own_line: true,
            item_finishes_with_line_break: true,
            inline_math_delimiters: InlineMathDelimiters::Dollars,
            display_math_delimiters: DisplayMathDelimiters::Dollars,
            display_math_layout: DisplayMathLayout::Adaptive,
        }
    }
}

impl WriterOptions {
    /// Options that reconstruct the input byte-for-byte.
    pub fn preserve() -> Self {
        Self {
            indent_width: 2,
            indent_environments: false,
            indent_document: false,
            trim_trailing_whitespace: false,
            max_blank_lines: None,
            line_ending: LineEnding::Preserve,
            ensure_final_newline: false,
            compact_math: false,
            simplify_math_scripts: false,
            sentence_per_line: false,
            collapse_prose_whitespace: false,
            blank_lines_around_sections: false,
            separate_math_environment_boundaries: false,
            align_environment_rows: false,
            item_starts_on_own_line: false,
            item_finishes_with_line_break: false,
            inline_math_delimiters: InlineMathDelimiters::Preserve,
            display_math_delimiters: DisplayMathDelimiters::Preserve,
            display_math_layout: DisplayMathLayout::Preserve,
        }
    }
}

/// Writes a parsed tree while retaining all source-backed content.
pub struct LatexWriter<'source, 'options> {
    source: &'source str,
    options: &'options WriterOptions,
}

impl<'source, 'options> LatexWriter<'source, 'options> {
    pub fn new(source: &'source str, options: &'options WriterOptions) -> Self {
        Self { source, options }
    }

    /// Walk the concrete tree and render it according to the configured policy.
    pub fn write(&self, tree: &Tree) -> Result<String, WriteError> {
        let root = tree.root_node();
        if root.start_byte() != 0 || root.end_byte() != self.source.len() {
            return Err(WriteError::TreeDoesNotCoverSource {
                tree: root.byte_range(),
                source_len: self.source.len(),
            });
        }

        let mut reconstructed = String::with_capacity(self.source.len());
        self.write_node(root, &mut reconstructed)?;

        if self.options == &WriterOptions::preserve() {
            return Ok(reconstructed);
        }

        let metadata = TreeMetadata::from_tree(tree, self.source, self.options);
        let tree_formatted = apply_tree_formatting(&reconstructed, self.options, &metadata)?;
        let mut parser = LatexParser::new()?;
        let formatted_tree = parser.parse(&tree_formatted)?;
        let item_formatted = apply_item_line_breaks(&tree_formatted, &formatted_tree, self.options);
        let item_tree = parser.parse(&item_formatted)?;
        let display_formatted =
            apply_display_math_layout(&item_formatted, &item_tree, self.options)?;
        let display_tree = parser.parse(&display_formatted)?;
        let environment_formatted =
            normalize_environment_boundaries(&display_formatted, &display_tree, self.options)?;
        let environment_tree = parser.parse(&environment_formatted)?;
        let metadata =
            TreeMetadata::from_tree(&environment_tree, &environment_formatted, self.options);
        let line_formatted = rewrite_lines(&environment_formatted, self.options, &metadata);
        let aligned = align_environment_rows(&line_formatted, self.options, &metadata);
        let block_formatted = separate_math_environment_boundaries(&aligned, self.options);
        Ok(rewrite_prose(&block_formatted, self.options))
    }

    fn write_node(&self, node: Node<'_>, output: &mut String) -> Result<(), WriteError> {
        let mut offset = node.start_byte();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            self.push_source(offset..child.start_byte(), output)?;
            if child.child_count() == 0 {
                self.push_source(child.byte_range(), output)?;
            } else {
                self.write_node(child, output)?;
            }
            offset = child.end_byte();
        }

        self.push_source(offset..node.end_byte(), output)
    }

    fn push_source(&self, range: Range<usize>, output: &mut String) -> Result<(), WriteError> {
        let text = self
            .source
            .get(range.clone())
            .ok_or(WriteError::InvalidSourceRange(range))?;
        output.push_str(text);
        Ok(())
    }
}

#[derive(Debug)]
pub enum WriteError {
    TreeDoesNotCoverSource {
        tree: Range<usize>,
        source_len: usize,
    },
    InvalidSourceRange(Range<usize>),
    OverlappingEdits,
    Parse(ParseError),
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TreeDoesNotCoverSource { tree, source_len } => write!(
                formatter,
                "tree byte range {tree:?} does not cover source range 0..{source_len}"
            ),
            Self::InvalidSourceRange(range) => {
                write!(
                    formatter,
                    "tree contains an invalid UTF-8 source range {range:?}"
                )
            }
            Self::OverlappingEdits => formatter.write_str("formatter produced overlapping edits"),
            Self::Parse(error) => write!(formatter, "could not reparse formatted LaTeX: {error}"),
        }
    }
}

impl Error for WriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParseError> for WriteError {
    fn from(error: ParseError) -> Self {
        Self::Parse(error)
    }
}

#[derive(Debug)]
struct TreeMetadata {
    environment_depth: Vec<usize>,
    display_math_depth: Vec<usize>,
    item_depth: Vec<usize>,
    raw_content: Vec<bool>,
    contains_comment: Vec<bool>,
    math_ranges: Vec<Range<usize>>,
    protected_math_ranges: Vec<Range<usize>>,
    simple_script_groups: Vec<Range<usize>>,
    formula_delimiters: Vec<FormulaDelimiters>,
    alignment_blocks: Vec<Range<usize>>,
}

impl TreeMetadata {
    fn from_tree(tree: &Tree, source: &str, options: &WriterOptions) -> Self {
        let line_count = source.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let mut metadata = Self {
            environment_depth: vec![0; line_count],
            display_math_depth: vec![0; line_count],
            item_depth: vec![0; line_count],
            raw_content: vec![false; line_count],
            contains_comment: vec![false; line_count],
            math_ranges: Vec::new(),
            protected_math_ranges: Vec::new(),
            simple_script_groups: Vec::new(),
            formula_delimiters: Vec::new(),
            alignment_blocks: Vec::new(),
        };
        metadata.visit(tree.root_node(), source, false, options);
        metadata.math_ranges.sort_by_key(|range| range.start);
        metadata
            .protected_math_ranges
            .sort_by_key(|range| range.start);
        metadata.math_ranges = merge_ranges(std::mem::take(&mut metadata.math_ranges));
        metadata.protected_math_ranges =
            merge_ranges(std::mem::take(&mut metadata.protected_math_ranges));
        metadata
    }

    fn visit(&mut self, node: Node<'_>, source: &str, in_math: bool, options: &WriterOptions) {
        if is_environment(node.kind()) {
            self.record_environment(node, source, options.indent_document);
        }

        let node_is_math = matches!(
            node.kind(),
            "inline_formula" | "displayed_equation" | "math_environment"
        );
        let in_math = in_math || node_is_math;

        match node.kind() {
            "inline_formula" => {
                self.math_ranges.push(node.byte_range());
                self.record_formula_delimiters(node, source, FormulaKind::Inline);
            }
            "displayed_equation" => {
                self.math_ranges.push(node.byte_range());
                self.record_formula_delimiters(node, source, FormulaKind::Display);
                if options.display_math_layout != DisplayMathLayout::Preserve {
                    self.record_display_math_depth(node);
                }
            }
            "enum_item" if options.item_finishes_with_line_break => {
                self.record_item_depth(node);
            }
            "math_environment" => {
                if let (Some(begin), Some(end)) = (
                    node.child_by_field_name("begin"),
                    node.child_by_field_name("end"),
                ) {
                    self.math_ranges.push(begin.end_byte()..end.start_byte());
                }
            }
            "text_mode" if in_math => {
                self.protected_math_ranges.push(node.byte_range());
            }
            "generic_command" if in_math && is_textual_math_command(source, node) => {
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind().starts_with("curly_group") {
                        self.protected_math_ranges.push(child.byte_range());
                    }
                }
            }
            "command_name" if in_math => {
                if source
                    .get(node.byte_range())
                    .is_some_and(|text| text.bytes().any(|byte| byte.is_ascii_whitespace()))
                {
                    self.protected_math_ranges.push(node.byte_range());
                }
            }
            "subscript" | "superscript" if in_math => {
                let field = if node.kind() == "subscript" {
                    "subscript"
                } else {
                    "superscript"
                };
                if let Some(group) = node.child_by_field_name(field)
                    && group.kind() == "curly_group"
                    && is_simple_script_group(source, group.byte_range())
                {
                    self.simple_script_groups.push(group.byte_range());
                }
            }
            _ => {}
        }

        if node.kind() == "line_comment" {
            let row = node.start_position().row;
            if let Some(value) = self.contains_comment.get_mut(row) {
                *value = true;
            }
            if in_math {
                self.protected_math_ranges.push(node.byte_range());
            }
        }

        let children_in_math = in_math && node.kind() != "text_mode";
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit(child, source, children_in_math, options);
        }
    }

    fn record_environment(&mut self, node: Node<'_>, source: &str, indent_document: bool) {
        let Some(begin) = node.child_by_field_name("begin") else {
            return;
        };
        let Some(end) = node.child_by_field_name("end") else {
            return;
        };

        let first_content_row = begin.end_position().row.saturating_add(1);
        let end_row = end.start_position().row;
        let environment_name = source
            .get(begin.byte_range())
            .and_then(|text| environment_marker(text, "begin"))
            .map(|(_, name)| name);
        let is_document = environment_name == Some("document");
        if indent_document || !is_document {
            for row in first_content_row..end_row {
                if let Some(depth) = self.environment_depth.get_mut(row) {
                    *depth += 1;
                }
            }
        }

        if environment_name.is_some_and(is_alignment_environment_name)
            && first_content_row < end_row
        {
            self.alignment_blocks.push(first_content_row..end_row);
        }

        if is_raw_environment(node.kind()) {
            for row in first_content_row..end_row {
                if let Some(value) = self.raw_content.get_mut(row) {
                    *value = true;
                }
            }
        }
    }

    fn record_display_math_depth(&mut self, node: Node<'_>) {
        let first_content_row = node.start_position().row.saturating_add(1);
        let closing_row = node.end_position().row;
        for row in first_content_row..closing_row {
            if let Some(depth) = self.display_math_depth.get_mut(row) {
                *depth += 1;
            }
        }
    }

    fn record_item_depth(&mut self, node: Node<'_>) {
        let Some(command) = node.child_by_field_name("command") else {
            return;
        };
        let declaration = node.child_by_field_name("label").unwrap_or(command);
        let first_content_row = declaration.end_position().row.saturating_add(1);
        let final_content_row = node.end_position().row;
        if first_content_row > final_content_row {
            return;
        }
        for row in first_content_row..=final_content_row {
            if let Some(depth) = self.item_depth.get_mut(row) {
                *depth += 1;
            }
        }
    }

    fn record_formula_delimiters(&mut self, node: Node<'_>, source: &str, kind: FormulaKind) {
        let range = node.byte_range();
        let Some(formula) = source.get(range.clone()) else {
            return;
        };
        let (opening_len, closing_len) = match kind {
            FormulaKind::Inline => {
                let opening_len = if formula.starts_with("\\(") {
                    2
                } else if formula.starts_with('$') {
                    1
                } else {
                    return;
                };
                let closing_len = if formula.ends_with("\\)") {
                    2
                } else if formula.ends_with('$') {
                    1
                } else {
                    return;
                };
                (opening_len, closing_len)
            }
            FormulaKind::Display => {
                if !(formula.starts_with("\\[") || formula.starts_with("$$"))
                    || !(formula.ends_with("\\]") || formula.ends_with("$$"))
                {
                    return;
                }
                (2, 2)
            }
        };
        if range.len() < opening_len + closing_len {
            return;
        }
        self.formula_delimiters.push(FormulaDelimiters {
            kind,
            opening: range.start..range.start + opening_len,
            closing: range.end - closing_len..range.end,
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum FormulaKind {
    Inline,
    Display,
}

#[derive(Debug)]
struct FormulaDelimiters {
    kind: FormulaKind,
    opening: Range<usize>,
    closing: Range<usize>,
}

fn is_environment(kind: &str) -> bool {
    kind == "generic_environment" || kind.ends_with("_environment")
}

fn is_raw_environment(kind: &str) -> bool {
    matches!(
        kind,
        "comment_environment"
            | "verbatim_environment"
            | "listing_environment"
            | "minted_environment"
            | "asy_environment"
            | "asydef_environment"
            | "pycode_environment"
            | "luacode_environment"
            | "sagesilent_environment"
            | "sageblock_environment"
    )
}

fn merge_ranges(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn is_simple_script_group(source: &str, range: Range<usize>) -> bool {
    let Some(group) = source.get(range.clone()) else {
        return false;
    };
    let Some(inner) = group
        .strip_prefix('{')
        .and_then(|text| text.strip_suffix('}'))
    else {
        return false;
    };
    let inner = inner.trim_matches([' ', '\t']);

    if inner.chars().count() == 1 {
        return inner.chars().next().is_some_and(char::is_alphanumeric);
    }

    let Some(command) = inner.strip_prefix('\\') else {
        return false;
    };
    if command.is_empty() {
        return false;
    }

    if command
        .chars()
        .all(|character| character.is_alphabetic() || character == '@')
    {
        let next = source[range.end..]
            .trim_start_matches([' ', '\t'])
            .chars()
            .next();
        return !next.is_some_and(|character| character.is_alphabetic() || character == '@');
    }

    command.chars().count() == 1
}

fn is_textual_math_command(source: &str, node: Node<'_>) -> bool {
    let Some(command) = node.child_by_field_name("command") else {
        return false;
    };
    let Some(name) = source.get(command.byte_range()) else {
        return false;
    };
    matches!(
        name,
        "\\text"
            | "\\textrm"
            | "\\textsf"
            | "\\texttt"
            | "\\textnormal"
            | "\\textbf"
            | "\\textmd"
            | "\\textit"
            | "\\textsl"
            | "\\textsc"
            | "\\emph"
            | "\\mbox"
            | "\\hbox"
            | "\\operatorname"
            | "\\operatorname*"
            | "\\mathrm"
            | "\\mathbf"
            | "\\mathit"
            | "\\mathsf"
            | "\\mathtt"
            | "\\mathcal"
            | "\\mathbb"
            | "\\mathfrak"
    )
}

#[derive(Debug)]
struct TextEdit {
    range: Range<usize>,
    replacement: &'static str,
}

fn apply_tree_formatting(
    source: &str,
    options: &WriterOptions,
    metadata: &TreeMetadata,
) -> Result<String, WriteError> {
    let mut edits = Vec::new();
    let skipped = SkipRegions::new(source);

    if options.compact_math {
        let bytes = source.as_bytes();
        for math in &metadata.math_ranges {
            let mut offset = math.start;
            while offset < math.end {
                if !matches!(bytes[offset], b' ' | b'\t') {
                    offset += 1;
                    continue;
                }

                let start = offset;
                while offset < math.end && matches!(bytes[offset], b' ' | b'\t') {
                    offset += 1;
                }
                let range = start..offset;
                if overlaps_any(&range, &metadata.protected_math_ranges) || skipped.overlaps(&range)
                {
                    continue;
                }

                let replacement = if needs_control_word_separator(source, start, offset) {
                    " "
                } else {
                    ""
                };
                if source.get(range.clone()) != Some(replacement) {
                    edits.push(TextEdit { range, replacement });
                }
            }
        }
    }

    if options.simplify_math_scripts {
        for group in &metadata.simple_script_groups {
            if skipped.overlaps(group) {
                continue;
            }
            edits.push(TextEdit {
                range: group.start..group.start + 1,
                replacement: "",
            });
            edits.push(TextEdit {
                range: group.end - 1..group.end,
                replacement: "",
            });
        }
    }

    for formula in &metadata.formula_delimiters {
        if skipped.overlaps(&formula.opening) || skipped.overlaps(&formula.closing) {
            continue;
        }
        let replacements = match formula.kind {
            FormulaKind::Inline => match options.inline_math_delimiters {
                InlineMathDelimiters::Preserve => None,
                InlineMathDelimiters::Dollars => Some(("$", "$")),
                InlineMathDelimiters::Parentheses => Some(("\\(", "\\)")),
            },
            FormulaKind::Display => match options.display_math_delimiters {
                DisplayMathDelimiters::Preserve => None,
                DisplayMathDelimiters::Dollars => Some(("$$", "$$")),
                DisplayMathDelimiters::Brackets => Some(("\\[", "\\]")),
            },
        };
        if let Some((opening, closing)) = replacements {
            if source.get(formula.opening.clone()) != Some(opening) {
                edits.push(TextEdit {
                    range: formula.opening.clone(),
                    replacement: opening,
                });
            }
            if source.get(formula.closing.clone()) != Some(closing) {
                edits.push(TextEdit {
                    range: formula.closing.clone(),
                    replacement: closing,
                });
            }
        }
    }

    edits.sort_by_key(|edit| edit.range.start);
    if edits
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err(WriteError::OverlappingEdits);
    }

    let mut output = source.to_owned();
    for edit in edits.into_iter().rev() {
        output.replace_range(edit.range, edit.replacement);
    }
    Ok(output)
}

#[derive(Debug)]
struct OwnedTextEdit {
    range: Range<usize>,
    replacement: String,
}

fn apply_item_line_breaks(source: &str, tree: &Tree, options: &WriterOptions) -> String {
    if !options.item_starts_on_own_line && !options.item_finishes_with_line_break {
        return source.to_owned();
    }

    let mut breaks = Vec::new();
    collect_item_line_breaks(tree.root_node(), source, options, &mut breaks);
    let skipped = SkipRegions::new(source);
    breaks.retain(|offset| !skipped.contains_offset(*offset));
    breaks.sort_unstable();
    breaks.dedup();

    let line_ending = detected_line_ending(source);
    let mut output = source.to_owned();
    for offset in breaks.into_iter().rev() {
        output.insert_str(offset, line_ending);
    }
    output
}

fn collect_item_line_breaks(
    node: Node<'_>,
    source: &str,
    options: &WriterOptions,
    breaks: &mut Vec<usize>,
) {
    if node.kind() == "enum_item"
        && let Some(command) = node.child_by_field_name("command")
    {
        if options.item_starts_on_own_line {
            let line_start = source[..command.start_byte()]
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            if !source[line_start..command.start_byte()]
                .trim_matches([' ', '\t', '\r'])
                .is_empty()
            {
                breaks.push(command.start_byte());
            }
        }

        if options.item_finishes_with_line_break {
            let declaration = node.child_by_field_name("label").unwrap_or(command);
            let line_end = source[declaration.end_byte()..]
                .find('\n')
                .map_or(source.len(), |newline| declaration.end_byte() + newline);
            if !source[declaration.end_byte()..line_end]
                .trim_matches([' ', '\t', '\r'])
                .is_empty()
            {
                breaks.push(declaration.end_byte());
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_item_line_breaks(child, source, options, breaks);
    }
}

fn apply_display_math_layout(
    source: &str,
    tree: &Tree,
    options: &WriterOptions,
) -> Result<String, WriteError> {
    if options.display_math_layout == DisplayMathLayout::Preserve {
        return Ok(source.to_owned());
    }

    let mut formulas = Vec::new();
    collect_display_formulas(tree.root_node(), &mut formulas);
    let skipped = SkipRegions::new(source);
    let line_ending = detected_line_ending(source);
    let mut edits = Vec::new();

    for formula in formulas {
        if skipped.overlaps(&formula.range) {
            continue;
        }
        if options.display_math_layout == DisplayMathLayout::Adaptive && formula.single_line {
            continue;
        }

        let range = formula.range;
        let text = source
            .get(range.clone())
            .ok_or_else(|| WriteError::InvalidSourceRange(range.clone()))?;
        let Some((opening_len, closing_len)) = display_delimiter_lengths(text) else {
            continue;
        };
        if text.len() < opening_len + closing_len {
            continue;
        }

        let opening = &text[..opening_len];
        let closing = &text[text.len() - closing_len..];
        let content =
            text[opening_len..text.len() - closing_len].trim_matches([' ', '\t', '\r', '\n']);
        let line_start = source[..range.start]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let line_end = source[range.end..]
            .find('\n')
            .map_or(source.len(), |newline| range.end + newline);
        let prefix_has_content = !source[line_start..range.start]
            .trim_matches([' ', '\t', '\r'])
            .is_empty();
        let suffix_has_content = !source[range.end..line_end]
            .trim_matches([' ', '\t', '\r'])
            .is_empty();

        let mut replacement = String::with_capacity(text.len() + line_ending.len() * 4);
        if prefix_has_content {
            replacement.push_str(line_ending);
        }
        replacement.push_str(opening);
        replacement.push_str(line_ending);
        if !content.is_empty() {
            replacement.push_str(content);
            replacement.push_str(line_ending);
        }
        replacement.push_str(closing);
        if suffix_has_content {
            replacement.push_str(line_ending);
        }
        edits.push(OwnedTextEdit { range, replacement });
    }

    edits.sort_by_key(|edit| edit.range.start);
    if edits
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err(WriteError::OverlappingEdits);
    }

    let mut output = source.to_owned();
    for edit in edits.into_iter().rev() {
        output.replace_range(edit.range, &edit.replacement);
    }
    Ok(output)
}

#[derive(Debug)]
struct DisplayFormula {
    range: Range<usize>,
    single_line: bool,
}

fn collect_display_formulas(node: Node<'_>, formulas: &mut Vec<DisplayFormula>) {
    if node.kind() == "displayed_equation" {
        formulas.push(DisplayFormula {
            range: node.byte_range(),
            single_line: node.start_position().row == node.end_position().row,
        });
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_display_formulas(child, formulas);
    }
}

fn normalize_environment_boundaries(
    source: &str,
    tree: &Tree,
    options: &WriterOptions,
) -> Result<String, WriteError> {
    let skipped = SkipRegions::new(source);
    let mut edits = Vec::new();
    collect_environment_edits(
        tree.root_node(),
        source,
        detected_line_ending(source),
        options,
        &skipped,
        &mut edits,
    );
    edits.sort_by_key(|edit| edit.range.start);
    if edits
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err(WriteError::OverlappingEdits);
    }

    let mut output = source.to_owned();
    for edit in edits.into_iter().rev() {
        output.replace_range(edit.range, &edit.replacement);
    }
    Ok(output)
}

fn collect_environment_edits(
    node: Node<'_>,
    source: &str,
    line_ending: &str,
    options: &WriterOptions,
    skipped: &SkipRegions,
    edits: &mut Vec<OwnedTextEdit>,
) {
    if is_environment(node.kind())
        && !is_raw_environment(node.kind())
        && let (Some(begin), Some(end)) = (
            node.child_by_field_name("begin"),
            node.child_by_field_name("end"),
        )
        && !skipped.overlaps(&begin.byte_range())
        && !skipped.overlaps(&end.byte_range())
    {
        let begin_start = begin.start_byte();
        let begin_end = begin.end_byte();
        let end_start = end.start_byte();
        let end_end = end.end_byte();
        let is_document = source
            .get(begin.byte_range())
            .and_then(|text| environment_marker(text, "begin"))
            .is_some_and(|(_, name)| name == "document");
        if let Some(options_node) = begin.child_by_field_name("options") {
            let options_start = options_node.start_byte();
            let name_end = begin
                .child_by_field_name("name")
                .map_or(options_start, |name| name.end_byte());
            let range = name_end..options_start;
            if source
                .get(range.clone())
                .is_some_and(|text| !text.is_empty() && text.chars().all(char::is_whitespace))
            {
                edits.push(OwnedTextEdit {
                    range,
                    replacement: String::new(),
                });
            }
        }
        let begin_line_start = source[..begin_start]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        if !source[begin_line_start..begin_start]
            .trim_matches([' ', '\t', '\r'])
            .is_empty()
            && !starts_parent_environment_body(node, source)
        {
            edits.push(OwnedTextEdit {
                range: begin_start..begin_start,
                replacement: line_ending.to_owned(),
            });
        }

        let end_line_end = source[end_end..]
            .find('\n')
            .map_or(source.len(), |newline| end_end + newline);
        if !source[end_end..end_line_end]
            .trim_matches([' ', '\t', '\r'])
            .is_empty()
            && !finishes_parent_environment_body(node, source)
        {
            edits.push(OwnedTextEdit {
                range: end_end..end_end,
                replacement: line_ending.to_owned(),
            });
        }

        if begin_end <= end_start && is_document {
            preserve_document_boundary_whitespace(
                source,
                begin_end,
                end_start,
                line_ending,
                skipped,
                edits,
            );
        } else if begin_end <= end_start {
            let mut cursor = skip_horizontal_and_line_whitespace(source, begin_end, end_start);
            let mut parameters = String::new();
            let mut labels = String::new();
            loop {
                let group_end = match source.as_bytes().get(cursor) {
                    Some(b'{') => grouped_command_end(source, cursor, b'{', b'}'),
                    Some(b'[') => grouped_command_end(source, cursor, b'[', b']'),
                    _ => None,
                };
                if let Some(group_end) = group_end.filter(|end| *end <= end_start) {
                    parameters.push_str(&source[cursor..group_end]);
                    cursor = skip_horizontal_and_line_whitespace(source, group_end, end_start);
                    continue;
                }
                if let Some(label_end) = label_command_end(source, cursor)
                    && label_end <= end_start
                {
                    labels.push_str(&source[cursor..label_end]);
                    cursor = skip_horizontal_and_line_whitespace(source, label_end, end_start);
                    continue;
                }
                break;
            }

            let body_end = cursor
                + source[cursor..end_start]
                    .trim_end_matches([' ', '\t', '\r', '\n'])
                    .len();
            let mut replacement = parameters;
            replacement.push_str(&labels);
            replacement.push_str(line_ending);
            let begin_range = begin_end..cursor;
            if !options.indent_environments
                && let Some(last_newline) = source[begin_range.clone()].rfind('\n')
            {
                replacement.push_str(&source[begin_end + last_newline + 1..cursor]);
            }

            if cursor >= body_end {
                let range = begin_end..end_start;
                if !skipped.overlaps(&range) {
                    edits.push(OwnedTextEdit { range, replacement });
                }
            } else {
                if !skipped.overlaps(&begin_range) {
                    edits.push(OwnedTextEdit {
                        range: begin_range,
                        replacement,
                    });
                }

                let end_range = body_end..end_start;
                if !skipped.overlaps(&end_range) {
                    let mut replacement = line_ending.to_owned();
                    if !options.indent_environments
                        && let Some(last_newline) = source[end_range.clone()].rfind('\n')
                    {
                        replacement.push_str(&source[body_end + last_newline + 1..end_start]);
                    }
                    edits.push(OwnedTextEdit {
                        range: end_range,
                        replacement,
                    });
                }
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_environment_edits(child, source, line_ending, options, skipped, edits);
    }
}

fn preserve_document_boundary_whitespace(
    source: &str,
    begin_end: usize,
    end_start: usize,
    line_ending: &str,
    skipped: &SkipRegions,
    edits: &mut Vec<OwnedTextEdit>,
) {
    let content_start = source[begin_end..end_start]
        .find(|character: char| !matches!(character, ' ' | '\t' | '\r' | '\n'))
        .map_or(end_start, |offset| begin_end + offset);
    let opening_whitespace = begin_end..content_start;
    if !source[opening_whitespace.clone()].contains('\n') && !skipped.overlaps(&opening_whitespace)
    {
        edits.push(OwnedTextEdit {
            range: opening_whitespace,
            replacement: line_ending.to_owned(),
        });
    }

    if content_start == end_start {
        return;
    }
    let content_end = content_start
        + source[content_start..end_start]
            .trim_end_matches([' ', '\t', '\r', '\n'])
            .len();
    let closing_whitespace = content_end..end_start;
    if !source[closing_whitespace.clone()].contains('\n') && !skipped.overlaps(&closing_whitespace)
    {
        edits.push(OwnedTextEdit {
            range: closing_whitespace,
            replacement: line_ending.to_owned(),
        });
    }
}

fn starts_parent_environment_body(node: Node<'_>, source: &str) -> bool {
    let Some(parent) = node.parent().filter(|parent| is_environment(parent.kind())) else {
        return false;
    };
    let Some(begin) = parent.child_by_field_name("begin") else {
        return false;
    };
    source[begin.end_byte()..node.start_byte()]
        .trim_matches([' ', '\t', '\r', '\n'])
        .is_empty()
}

fn finishes_parent_environment_body(node: Node<'_>, source: &str) -> bool {
    let Some(parent) = node.parent().filter(|parent| is_environment(parent.kind())) else {
        return false;
    };
    let Some(end) = parent.child_by_field_name("end") else {
        return false;
    };
    source[node.end_byte()..end.start_byte()]
        .trim_matches([' ', '\t', '\r', '\n'])
        .is_empty()
}

fn skip_horizontal_and_line_whitespace(source: &str, mut offset: usize, end: usize) -> usize {
    while offset < end && matches!(source.as_bytes()[offset], b' ' | b'\t' | b'\r' | b'\n') {
        offset += 1;
    }
    offset
}

fn display_delimiter_lengths(formula: &str) -> Option<(usize, usize)> {
    let opening_len = (formula.starts_with("$$") || formula.starts_with("\\[")).then_some(2)?;
    let closing_len = (formula.ends_with("$$") || formula.ends_with("\\]")).then_some(2)?;
    Some((opening_len, closing_len))
}

fn overlaps_any(range: &Range<usize>, protected: &[Range<usize>]) -> bool {
    protected
        .iter()
        .any(|item| range.start < item.end && item.start < range.end)
}

fn needs_control_word_separator(
    source: &str,
    whitespace_start: usize,
    whitespace_end: usize,
) -> bool {
    let Some(next) = source
        .get(whitespace_end..)
        .and_then(|text| text.chars().next())
    else {
        return false;
    };
    if !(next.is_alphabetic() || next == '@') {
        return false;
    }

    let bytes = source.as_bytes();
    let mut start = whitespace_start;
    while start > 0 && (bytes[start - 1].is_ascii_alphabetic() || bytes[start - 1] == b'@') {
        start -= 1;
    }
    start < whitespace_start && start > 0 && bytes[start - 1] == b'\\'
}

#[derive(Debug)]
struct OwnedSourceLine {
    content: String,
    ending: String,
}

#[derive(Debug)]
struct AlignmentRow {
    line: usize,
    leading: String,
    cells: Vec<String>,
    suffix: String,
    has_terminator: bool,
}

fn align_environment_rows(
    source: &str,
    options: &WriterOptions,
    metadata: &TreeMetadata,
) -> String {
    if !options.align_environment_rows || metadata.alignment_blocks.is_empty() {
        return source.to_owned();
    }

    let mut lines = SourceLines::new(source)
        .map(|line| OwnedSourceLine {
            content: line.content.to_owned(),
            ending: line.ending.to_owned(),
        })
        .collect::<Vec<_>>();
    let skipped = SkipRegions::new(source);

    for (block_index, block) in metadata.alignment_blocks.iter().enumerate() {
        let mut rows = block
            .clone()
            .filter(|row| {
                !metadata
                    .alignment_blocks
                    .iter()
                    .enumerate()
                    .any(|(other_index, other)| {
                        other_index != block_index
                            && other.start >= block.start
                            && other.end <= block.end
                            && (other.start != block.start || other.end != block.end)
                            && other.contains(row)
                    })
            })
            .filter(|row| !skipped.contains_row(*row))
            .filter_map(|row| {
                lines
                    .get(row)
                    .and_then(|line| parse_alignment_row(row, &line.content))
            })
            .collect::<Vec<_>>();

        if rows.is_empty() {
            continue;
        }

        let column_count = rows
            .iter()
            .map(|row| row.cells.len().saturating_sub(1))
            .max()
            .unwrap_or(0);
        let mut column_widths = vec![0; column_count];
        for row in &rows {
            for (column, cell) in row.cells.iter().take(column_count).enumerate() {
                if column + 1 < row.cells.len() {
                    column_widths[column] = column_widths[column].max(display_width(cell));
                }
            }
        }

        let rendered_bodies = rows
            .iter()
            .map(|row| render_alignment_cells(&row.cells, &column_widths))
            .collect::<Vec<_>>();
        let terminator_column = rendered_bodies
            .iter()
            .map(|body| display_width(body))
            .max()
            .unwrap_or(0)
            + 1;

        for (row, body) in rows.drain(..).zip(rendered_bodies) {
            let mut content = row.leading;
            content.push_str(&body);
            if row.has_terminator {
                content.extend(std::iter::repeat_n(
                    ' ',
                    terminator_column.saturating_sub(display_width(&body)),
                ));
            }
            content.push_str(&row.suffix);
            lines[row.line].content = content;
        }
    }

    let mut output = String::with_capacity(source.len());
    for line in lines {
        output.push_str(&line.content);
        output.push_str(&line.ending);
    }
    output
}

fn parse_alignment_row(line: usize, content: &str) -> Option<AlignmentRow> {
    if content.contains("\\verb") {
        return None;
    }

    let comment_start = find_unescaped_percent(content).unwrap_or(content.len());
    let code = &content[..comment_start];
    let terminator_start = trailing_row_terminator_start(code);
    let body_end = terminator_start.unwrap_or_else(|| code.trim_end_matches([' ', '\t']).len());
    let body = &content[..body_end];
    let leading_len = body.len() - body.trim_start_matches([' ', '\t']).len();
    let leading = &body[..leading_len];
    let core = &body[leading_len..];
    let separators = top_level_ampersands(core);
    if separators.is_empty() && terminator_start.is_none() {
        return None;
    }

    let mut cells = Vec::with_capacity(separators.len() + 1);
    let mut start = 0;
    for separator in separators {
        let cell = core[start..separator].trim_end_matches([' ', '\t']);
        cells.push(if cells.is_empty() {
            cell.to_owned()
        } else {
            cell.trim_start_matches([' ', '\t']).to_owned()
        });
        start = separator + 1;
    }
    let last = core[start..].trim_end_matches([' ', '\t']);
    cells.push(if cells.is_empty() {
        last.to_owned()
    } else {
        last.trim_start_matches([' ', '\t']).to_owned()
    });

    let suffix = if terminator_start.is_some() && comment_start < content.len() {
        format!(
            "{} {}",
            content[body_end..comment_start].trim_end_matches([' ', '\t']),
            &content[comment_start..]
        )
    } else {
        content[body_end..].to_owned()
    };

    Some(AlignmentRow {
        line,
        leading: leading.to_owned(),
        cells,
        suffix,
        has_terminator: terminator_start.is_some(),
    })
}

fn render_alignment_cells(cells: &[String], column_widths: &[usize]) -> String {
    let mut output = String::new();
    for (column, cell) in cells.iter().enumerate() {
        output.push_str(cell);
        if column + 1 < cells.len() {
            output.extend(std::iter::repeat_n(
                ' ',
                column_widths[column].saturating_sub(display_width(cell)) + 1,
            ));
            output.push_str("& ");
        }
    }
    output
}

fn display_width(text: &str) -> usize {
    text.chars().count()
}

fn find_unescaped_percent(text: &str) -> Option<usize> {
    text.bytes().enumerate().find_map(|(index, byte)| {
        (byte == b'%' && !is_escaped(text.as_bytes(), index)).then_some(index)
    })
}

fn top_level_ampersands(text: &str) -> Vec<usize> {
    let mut depth = 0usize;
    let mut separators = Vec::new();
    for (index, byte) in text.bytes().enumerate() {
        if is_escaped(text.as_bytes(), index) {
            continue;
        }
        match byte {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b'&' if depth == 0 => separators.push(index),
            _ => {}
        }
    }
    separators
}

fn trailing_row_terminator_start(text: &str) -> Option<usize> {
    let trimmed = text.trim_end_matches([' ', '\t']);
    let command_end = trailing_bracket_group_start(trimmed).unwrap_or(trimmed.len());
    let command = trimmed[..command_end].trim_end_matches([' ', '\t']);
    let start = if command.ends_with("\\\\*") {
        command.len() - 3
    } else if command.ends_with("\\\\") {
        command.len() - 2
    } else {
        return None;
    };

    (!is_escaped(command.as_bytes(), start) && brace_depth_at(command, start) == 0).then_some(start)
}

fn trailing_bracket_group_start(text: &str) -> Option<usize> {
    if !text.ends_with(']') || is_escaped(text.as_bytes(), text.len() - 1) {
        return None;
    }

    let mut depth = 0;
    for (index, byte) in text.bytes().enumerate().rev() {
        if is_escaped(text.as_bytes(), index) {
            continue;
        }
        match byte {
            b']' => depth += 1,
            b'[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn brace_depth_at(text: &str, end: usize) -> usize {
    let mut depth = 0usize;
    for (index, byte) in text[..end].bytes().enumerate() {
        if is_escaped(text.as_bytes(), index) {
            continue;
        }
        match byte {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth
}

fn rewrite_lines(source: &str, options: &WriterOptions, metadata: &TreeMetadata) -> String {
    let fallback_ending = detected_line_ending(source);
    let skipped = SkipRegions::new(source);
    let mut output = String::with_capacity(source.len());
    let mut blank_lines = 0;

    for (row, line) in SourceLines::new(source).enumerate() {
        if skipped.contains_row(row) {
            output.push_str(line.content);
            output.push_str(line.ending);
            blank_lines = 0;
            continue;
        }
        let raw_content = metadata.raw_content.get(row).copied().unwrap_or(false);
        let contains_comment = metadata.contains_comment.get(row).copied().unwrap_or(false);
        let mut content = line.content;

        let blank = content.trim_matches([' ', '\t']).is_empty();
        if blank && !raw_content {
            blank_lines += 1;
            if options
                .max_blank_lines
                .is_some_and(|maximum| blank_lines > maximum)
            {
                continue;
            }
        } else {
            blank_lines = 0;
        }

        if options.indent_environments && !raw_content && !blank {
            content = content.trim_start_matches([' ', '\t']);
            let depth = metadata.environment_depth.get(row).copied().unwrap_or(0)
                + metadata.display_math_depth.get(row).copied().unwrap_or(0)
                + metadata.item_depth.get(row).copied().unwrap_or(0);
            output.extend(std::iter::repeat_n(' ', depth * options.indent_width));
        }

        if options.trim_trailing_whitespace && !raw_content && !contains_comment {
            content = content.trim_end_matches([' ', '\t']);
        }

        output.push_str(content);
        if !line.ending.is_empty() {
            output.push_str(match options.line_ending {
                LineEnding::Preserve => line.ending,
                LineEnding::Lf => "\n",
                LineEnding::Crlf => "\r\n",
            });
        }
    }

    if options.ensure_final_newline
        && !skipped.rows.last().copied().unwrap_or(false)
        && !output.is_empty()
        && !output.ends_with('\n')
    {
        output.push_str(match options.line_ending {
            LineEnding::Preserve => fallback_ending,
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        });
    }

    output
}

fn separate_math_environment_boundaries(source: &str, options: &WriterOptions) -> String {
    if !options.separate_math_environment_boundaries {
        return source.to_owned();
    }

    let fallback_ending = detected_line_ending(source);
    let skipped = SkipRegions::new(source);
    let mut output = String::with_capacity(source.len());

    for (row, line) in SourceLines::new(source).enumerate() {
        if skipped.contains_row(row) {
            output.push_str(line.content);
            output.push_str(line.ending);
            continue;
        }
        let leading_len = line.content.len() - line.content.trim_start_matches([' ', '\t']).len();
        let leading = &line.content[..leading_len];
        let trimmed = &line.content[leading_len..];
        let inserted_ending = if line.ending.is_empty() {
            fallback_ending
        } else {
            line.ending
        };

        if let Some((marker_end, name)) = environment_marker(trimmed, "end")
            && is_math_environment_name(name)
            && !trimmed[marker_end..].trim().is_empty()
        {
            output.push_str(leading);
            output.push_str(&trimmed[..marker_end]);
            output.push_str(inserted_ending);
            output.push_str(leading);
            output.push_str(trimmed[marker_end..].trim_start_matches([' ', '\t']));
            output.push_str(line.ending);
            continue;
        }

        if let Some(begin_offset) = trimmed.find("\\begin{")
            && begin_offset > 0
            && !trimmed[..begin_offset].trim().is_empty()
            && let Some((_, name)) = environment_marker(&trimmed[begin_offset..], "begin")
            && is_math_environment_name(name)
        {
            output.push_str(leading);
            output.push_str(trimmed[..begin_offset].trim_end_matches([' ', '\t']));
            output.push_str(inserted_ending);
            output.push_str(leading);
            output.push_str(&trimmed[begin_offset..]);
            output.push_str(line.ending);
            continue;
        }

        output.push_str(line.content);
        output.push_str(line.ending);
    }

    output
}

fn environment_marker<'line>(line: &'line str, marker: &str) -> Option<(usize, &'line str)> {
    let prefix = match marker {
        "begin" => "\\begin{",
        "end" => "\\end{",
        _ => return None,
    };
    let rest = line.strip_prefix(prefix)?;
    let close = rest.find('}')?;
    Some((prefix.len() + close + 1, &rest[..close]))
}

fn is_math_environment_name(name: &str) -> bool {
    matches!(
        name,
        "math"
            | "displaymath"
            | "displaymath*"
            | "equation"
            | "equation*"
            | "multline"
            | "multline*"
            | "eqnarray"
            | "eqnarray*"
            | "align"
            | "align*"
            | "aligned"
            | "aligned*"
            | "array"
            | "array*"
            | "split"
            | "split*"
            | "alignat"
            | "alignat*"
            | "alignedat"
            | "alignedat*"
            | "gather"
            | "gather*"
            | "gathered"
            | "gathered*"
            | "flalign"
            | "flalign*"
    )
}

fn is_alignment_environment_name(name: &str) -> bool {
    matches!(
        name,
        "align"
            | "align*"
            | "alignat"
            | "alignat*"
            | "aligned"
            | "aligned*"
            | "alignedat"
            | "alignedat*"
            | "flalign"
            | "flalign*"
            | "eqnarray"
            | "eqnarray*"
            | "gather"
            | "gather*"
            | "gathered"
            | "gathered*"
            | "multline"
            | "multline*"
            | "split"
            | "array"
            | "array*"
            | "cases"
            | "matrix"
            | "matrix*"
            | "pmatrix"
            | "pmatrix*"
            | "bmatrix"
            | "bmatrix*"
            | "Bmatrix"
            | "Bmatrix*"
            | "vmatrix"
            | "vmatrix*"
            | "Vmatrix"
            | "Vmatrix*"
            | "smallmatrix"
            | "tabular"
            | "tabular*"
            | "tabularx"
            | "longtable"
            | "tblr"
            | "longtblr"
            | "talltblr"
    )
}

fn rewrite_prose(source: &str, options: &WriterOptions) -> String {
    if !options.sentence_per_line
        && !options.blank_lines_around_sections
        && !options.collapse_prose_whitespace
    {
        return source.to_owned();
    }

    let fallback_ending = detected_line_ending(source);
    let skipped = SkipRegions::new(source);
    let mut output = String::with_capacity(source.len());
    let mut previous_blank = true;
    let mut previous_section = false;
    let mut raw_environment: Option<String> = None;
    let mut structured_environments = Vec::new();

    let lines = SourceLines::new(source).collect::<Vec<_>>();
    let mut row = 0;
    while let Some(line) = lines.get(row) {
        if skipped.contains_row(row) {
            output.push_str(line.content);
            output.push_str(line.ending);
            previous_blank = false;
            previous_section = false;
            if let Some(name) = raw_environment_name(line.content, "begin") {
                raw_environment = Some(name.to_owned());
            }
            if let Some(name) = raw_environment.as_deref()
                && raw_environment_name(line.content, "end") == Some(name)
            {
                raw_environment = None;
            }
            if raw_environment.is_none() {
                update_structured_environments(line.content, &mut structured_environments);
            }
            row += 1;
            continue;
        }
        let blank = line.content.trim_matches([' ', '\t']).is_empty();
        let entering_raw = raw_environment_name(line.content, "begin");
        let raw_content = raw_environment.is_some() || entering_raw.is_some();
        let section_end = (!raw_content)
            .then(|| section_command_end(line.content))
            .flatten();
        let section = section_end.is_some();
        let mut attached_label = None;
        let mut consumed_row = row;
        if let Some(end) = section_end
            && line.content[end..].trim_matches([' ', '\t']).is_empty()
        {
            let mut next = row + 1;
            while let Some(candidate) = lines.get(next)
                && !skipped.contains_row(next)
                && candidate.content.trim_matches([' ', '\t']).is_empty()
            {
                next += 1;
            }
            if let Some(candidate) = lines.get(next)
                && !skipped.contains_row(next)
                && let Some(label) = standalone_label(candidate.content)
            {
                attached_label = Some(label);
                consumed_row = next;
            }
        }
        let inserted_ending = if line.ending.is_empty() {
            fallback_ending
        } else {
            line.ending
        };

        if options.blank_lines_around_sections {
            if previous_section && !blank {
                output.push_str(inserted_ending);
                previous_blank = true;
            }
            if section && !previous_blank && !output.is_empty() {
                if !output.ends_with('\n') {
                    output.push_str(inserted_ending);
                }
                output.push_str(inserted_ending);
            }
        }

        let mut content = line.content;
        let mut section_has_body = false;
        if let Some(end) = section_end {
            let body = line.content[end..].trim_start_matches([' ', '\t']);
            if !body.is_empty() && !body.starts_with('%') {
                output.push_str(line.content[..end].trim_end_matches([' ', '\t']));
                output.push_str(inserted_ending);
                if options.blank_lines_around_sections {
                    output.push_str(inserted_ending);
                }
                content = body;
                section_has_body = true;
            }
        }

        let normalized_content = (options.collapse_prose_whitespace
            && !raw_content
            && structured_environments.is_empty()
            && is_prose_line(content))
        .then(|| collapse_prose_whitespace(content));
        let content = normalized_content.as_deref().unwrap_or(content);

        let boundaries = if options.sentence_per_line
            && !raw_content
            && structured_environments.is_empty()
            && is_prose_line(content)
        {
            sentence_boundaries(content)
        } else {
            Vec::new()
        };
        let leading_len = content.len() - content.trim_start_matches([' ', '\t']).len();
        let leading = &content[..leading_len];
        let mut start = 0;
        for boundary in boundaries {
            output.push_str(content[start..boundary].trim_end_matches([' ', '\t']));
            output.push_str(inserted_ending);
            output.push_str(leading);
            start = boundary;
            while content[start..].starts_with([' ', '\t']) {
                start += 1;
            }
        }
        output.push_str(&content[start..]);
        if let Some(label) = attached_label {
            output.push_str(label);
        }
        output.push_str(lines[consumed_row].ending);

        previous_blank = blank;
        previous_section = section && !section_has_body;

        if let Some(name) = entering_raw {
            raw_environment = Some(name.to_owned());
        }
        if let Some(name) = raw_environment.as_deref()
            && raw_environment_name(line.content, "end") == Some(name)
        {
            raw_environment = None;
        }
        if !raw_content {
            update_structured_environments(line.content, &mut structured_environments);
        }
        row = consumed_row + 1;
    }

    output
}

fn section_command_end(line: &str) -> Option<usize> {
    if line.starts_with([' ', '\t']) {
        return None;
    }
    let command = line.strip_prefix('\\')?;
    let name_len = command
        .find(|character: char| !(character.is_ascii_alphabetic() || character == '*'))
        .unwrap_or(command.len());
    if !matches!(
        &command[..name_len],
        "part"
            | "chapter"
            | "section"
            | "section*"
            | "subsection"
            | "subsection*"
            | "subsubsection"
            | "subsubsection*"
            | "paragraph"
            | "paragraph*"
            | "subparagraph"
            | "subparagraph*"
    ) {
        return None;
    }

    let bytes = line.as_bytes();
    let mut offset = 1 + name_len;
    while bytes
        .get(offset)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        offset += 1;
    }
    if bytes.get(offset) == Some(&b'[') {
        offset = grouped_command_end(line, offset, b'[', b']')?;
        while bytes
            .get(offset)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            offset += 1;
        }
    }
    if bytes.get(offset) != Some(&b'{') {
        return None;
    }
    offset = grouped_command_end(line, offset, b'{', b'}')?;
    loop {
        let label_start = line.len() - line[offset..].trim_start_matches([' ', '\t']).len();
        let Some(label_end) = label_command_end(line, label_start) else {
            break;
        };
        offset = label_end;
    }
    Some(offset)
}

fn label_command_end(line: &str, start: usize) -> Option<usize> {
    let command = line.get(start..)?;
    command.strip_prefix("\\label{")?;
    grouped_command_end(line, start + "\\label".len(), b'{', b'}')
}

fn standalone_label(line: &str) -> Option<&str> {
    let content = line.trim_matches([' ', '\t']);
    let mut offset = 0;
    loop {
        offset = label_command_end(content, offset)?;
        offset = content.len() - content[offset..].trim_start_matches([' ', '\t']).len();
        if offset == content.len() {
            return Some(content);
        }
    }
}

fn grouped_command_end(line: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut depth = 0;
    for (offset, byte) in bytes.iter().enumerate().skip(start) {
        if is_escaped(bytes, offset) {
            continue;
        }
        if *byte == open {
            depth += 1;
        } else if *byte == close {
            depth -= 1;
            if depth == 0 {
                return Some(offset + 1);
            }
        }
    }
    None
}

fn is_prose_line(line: &str) -> bool {
    let content = line.trim_start_matches([' ', '\t']);
    !content.is_empty()
        && !content.starts_with(['\\', '%'])
        && content.chars().any(char::is_alphabetic)
}

fn update_structured_environments(line: &str, environments: &mut Vec<String>) {
    let trimmed = line.trim_start_matches([' ', '\t']);
    if let Some((_, name)) = environment_marker(trimmed, "begin")
        && (is_math_environment_name(name) || is_alignment_environment_name(name))
    {
        environments.push(name.to_owned());
    }
    if let Some((_, name)) = environment_marker(trimmed, "end")
        && environments.last().is_some_and(|open| open == name)
    {
        environments.pop();
    }
}

fn raw_environment_name<'line>(line: &'line str, marker: &str) -> Option<&'line str> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let prefix = match marker {
        "begin" => "\\begin{",
        "end" => "\\end{",
        _ => return None,
    };
    let rest = trimmed.strip_prefix(prefix)?;
    let name = rest.split_once('}')?.0;
    matches!(
        name,
        "comment"
            | "verbatim"
            | "Verbatim"
            | "lstlisting"
            | "minted"
            | "asy"
            | "asydef"
            | "pycode"
            | "luacode"
            | "luacode*"
            | "sagesilent"
            | "sageblock"
    )
    .then_some(name)
}

fn collapse_prose_whitespace(line: &str) -> String {
    if line.contains("\\verb") || line.contains("\\lstinline") || line.contains("\\mintinline") {
        return line.to_owned();
    }

    let bytes = line.as_bytes();
    let leading_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let mut output = String::with_capacity(line.len());
    output.push_str(&line[..leading_len]);
    let mut offset = leading_len;
    let mut in_math = false;
    let mut brace_depth = 0usize;

    while offset < bytes.len() {
        if bytes[offset] == b'\\'
            && !is_escaped(bytes, offset)
            && offset + 1 < bytes.len()
            && matches!(bytes[offset + 1], b'(' | b'[' | b')' | b']')
        {
            in_math = matches!(bytes[offset + 1], b'(' | b'[');
            output.push_str(&line[offset..offset + 2]);
            offset += 2;
            continue;
        }
        if bytes[offset] == b'$' && !is_escaped(bytes, offset) {
            let length = if bytes.get(offset + 1) == Some(&b'$') {
                2
            } else {
                1
            };
            in_math = !in_math;
            output.push_str(&line[offset..offset + length]);
            offset += length;
            continue;
        }
        if bytes[offset] == b'%' && !is_escaped(bytes, offset) && !in_math {
            output.push_str(&line[offset..]);
            break;
        }
        if !in_math && !is_escaped(bytes, offset) {
            match bytes[offset] {
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
        if !in_math && brace_depth == 0 && matches!(bytes[offset], b' ' | b'\t') {
            while offset < bytes.len() && matches!(bytes[offset], b' ' | b'\t') {
                offset += 1;
            }
            output.push(' ');
            continue;
        }

        let character = line[offset..].chars().next().unwrap();
        output.push(character);
        offset += character.len_utf8();
    }

    output
}

fn sentence_boundaries(line: &str) -> Vec<usize> {
    let bytes = line.as_bytes();
    let mut boundaries = Vec::new();
    let mut in_math = false;
    let mut brace_depth: usize = 0;
    let mut offset = 0;

    while offset < bytes.len() {
        if bytes[offset] == b'%' && !is_escaped(bytes, offset) && !in_math {
            break;
        }
        if bytes[offset] == b'\\'
            && !is_escaped(bytes, offset)
            && offset + 1 < bytes.len()
            && matches!(bytes[offset + 1], b'(' | b'[')
        {
            in_math = true;
            offset += 2;
            continue;
        }
        if bytes[offset] == b'\\'
            && !is_escaped(bytes, offset)
            && offset + 1 < bytes.len()
            && matches!(bytes[offset + 1], b')' | b']')
        {
            in_math = false;
            offset += 2;
            continue;
        }
        if bytes[offset] == b'$' && !is_escaped(bytes, offset) {
            in_math = !in_math;
            offset += if offset + 1 < bytes.len() && bytes[offset + 1] == b'$' {
                2
            } else {
                1
            };
            continue;
        }
        if !in_math && !is_escaped(bytes, offset) {
            match bytes[offset] {
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
        if bytes[offset] != b'.' || in_math || brace_depth > 0 || is_abbreviation(line, offset) {
            offset += 1;
            continue;
        }

        let mut next = offset + 1;
        while next < bytes.len() && matches!(bytes[next], b'"' | b'\'' | b')' | b']') {
            next += 1;
        }
        let whitespace_start = next;
        while next < bytes.len() && matches!(bytes[next], b' ' | b'\t') {
            next += 1;
        }
        if next > whitespace_start
            && next < bytes.len()
            && (line[next..].chars().next().is_some_and(char::is_uppercase)
                || line[next..].starts_with('$')
                || line[next..].starts_with("\\("))
        {
            boundaries.push(next);
            offset = next;
        } else {
            offset += 1;
        }
    }

    boundaries
}

fn is_escaped(bytes: &[u8], offset: usize) -> bool {
    let mut slashes = 0;
    let mut cursor = offset;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        slashes += 1;
        cursor -= 1;
    }
    slashes % 2 == 1
}

fn is_abbreviation(line: &str, period: usize) -> bool {
    let prefix = &line[..period];
    let word_start = prefix
        .rfind(|character: char| !(character.is_alphabetic() || character == '.'))
        .map_or(0, |index| index + 1);
    matches!(
        prefix[word_start..].to_ascii_lowercase().as_str(),
        "cf" | "e.g" | "i.e" | "etc" | "mr" | "mrs" | "ms" | "dr" | "prof" | "vs"
    )
}

fn detected_line_ending(source: &str) -> &'static str {
    let bytes = source.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            return if index > 0 && bytes[index - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            };
        }
    }
    "\n"
}

#[derive(Debug)]
struct SourceLine<'source> {
    content: &'source str,
    ending: &'source str,
}

struct SourceLines<'source> {
    source: &'source str,
    offset: usize,
}

/// Full-line comment directives protect their own lines and the lines between them.
struct SkipRegions {
    ranges: Vec<Range<usize>>,
    rows: Vec<bool>,
}

impl SkipRegions {
    fn new(source: &str) -> Self {
        let mut ranges = Vec::new();
        let mut rows = Vec::new();
        let mut active: Option<(usize, usize)> = None;
        let mut raw_environment: Option<String> = None;
        let mut offset = 0;

        for (row, line) in SourceLines::new(source).enumerate() {
            rows.push(false);
            let directive = line.content.trim_matches([' ', '\t']);
            let end = offset + line.content.len() + line.ending.len();
            let entering_raw = raw_environment_name(line.content, "begin");
            let is_comment_directive = raw_environment.is_none() && entering_raw.is_none();

            if let Some((start, first_row)) = active {
                if is_comment_directive && directive == "% latex-treefmt: on" {
                    ranges.push(start..end);
                    rows[first_row..=row].fill(true);
                    active = None;
                }
            } else if is_comment_directive && directive == "% latex-treefmt: off" {
                active = Some((offset, row));
            }

            if let Some(name) = raw_environment.as_deref()
                && raw_environment_name(line.content, "end") == Some(name)
            {
                raw_environment = None;
            } else if raw_environment.is_none()
                && let Some(name) = entering_raw
            {
                raw_environment = Some(name.to_owned());
            }

            offset = end;
        }

        if let Some((start, first_row)) = active {
            ranges.push(start..source.len());
            rows[first_row..].fill(true);
        }

        Self { ranges, rows }
    }

    fn contains_row(&self, row: usize) -> bool {
        self.rows.get(row).copied().unwrap_or(false)
    }

    fn contains_offset(&self, offset: usize) -> bool {
        self.ranges.iter().any(|range| range.contains(&offset))
    }

    fn overlaps(&self, range: &Range<usize>) -> bool {
        overlaps_any(range, &self.ranges)
    }
}

impl<'source> SourceLines<'source> {
    fn new(source: &'source str) -> Self {
        Self { source, offset: 0 }
    }
}

impl<'source> Iterator for SourceLines<'source> {
    type Item = SourceLine<'source>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.source.len() {
            return None;
        }

        let remaining = &self.source[self.offset..];
        let newline = remaining.find('\n');
        let end = newline.map_or(self.source.len(), |relative| self.offset + relative + 1);
        let raw = &self.source[self.offset..end];
        self.offset = end;

        if let Some(without_lf) = raw.strip_suffix('\n') {
            if let Some(content) = without_lf.strip_suffix('\r') {
                Some(SourceLine {
                    content,
                    ending: "\r\n",
                })
            } else {
                Some(SourceLine {
                    content: without_lf,
                    ending: "\n",
                })
            }
        } else {
            Some(SourceLine {
                content: raw,
                ending: "",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        DisplayMathDelimiters, DisplayMathLayout, InlineMathDelimiters, LatexParser, LatexWriter,
        LineEnding, WriterOptions,
    };

    fn write(source: &str, options: &WriterOptions) -> String {
        let mut parser = LatexParser::new().unwrap();
        let tree = parser.parse(source).unwrap();
        LatexWriter::new(source, options).write(&tree).unwrap()
    }

    #[test]
    fn preserve_mode_is_byte_for_byte_lossless() {
        let source = "Text $\\sqrt[3]{x}$  % keep me  \r\n";
        assert_eq!(write(source, &WriterOptions::preserve()), source);
    }

    #[test]
    fn indents_nested_environments_and_trims_lines() {
        let source = concat!(
            "\\begin{document}\n",
            " text   \n",
            " \\begin{itemize}\n",
            "\\item one   \n",
            "\n",
            "\n",
            " \\end{itemize}\n",
            "\\end{document}"
        );
        let expected = concat!(
            "\\begin{document}\n",
            "text\n",
            "\\begin{itemize}\n",
            "  \\item\n",
            "    one\n",
            "\\end{itemize}\n",
            "\\end{document}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn puts_item_declarations_and_bodies_on_separate_lines() {
        let source = concat!(
            "\\begin{enumerate}\n",
            "prefix \\item First item. More text.\\item[Named] Second item.\n",
            "\\end{enumerate}\n"
        );
        let expected = concat!(
            "\\begin{enumerate}\n",
            "  prefix\n",
            "  \\item\n",
            "    First item.\n",
            "    More text.\n",
            "  \\item[Named]\n",
            "    Second item.\n",
            "\\end{enumerate}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn can_preserve_existing_item_line_breaks() {
        let source = "\\begin{enumerate}\n\\item One\n\\end{enumerate}\n";
        let options = WriterOptions {
            item_starts_on_own_line: false,
            item_finishes_with_line_break: false,
            ..WriterOptions::default()
        };

        assert_eq!(
            write(source, &options),
            "\\begin{enumerate}\n  \\item One\n\\end{enumerate}\n"
        );
    }

    #[test]
    fn can_indent_document_contents() {
        let source = "\\begin{document}\ntext\n\\end{document}\n";
        let options = WriterOptions {
            indent_document: true,
            ..WriterOptions::default()
        };

        assert_eq!(
            write(source, &options),
            "\\begin{document}\n  text\n\\end{document}\n"
        );
    }

    #[test]
    fn allows_blank_lines_inside_document_boundaries() {
        let source = concat!(
            "\\begin{document}\n",
            "\n",
            " text   \n",
            "\n",
            "\\end{document}\n"
        );
        let expected = concat!(
            "\\begin{document}\n",
            "\n",
            "text\n",
            "\n",
            "\\end{document}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn puts_inline_document_boundaries_on_their_own_lines() {
        let source = "Before \\begin{document}Body text.\\end{document} After\n";
        let expected = concat!(
            "Before\n",
            "\\begin{document}\n",
            "Body text.\n",
            "\\end{document}\n",
            "After\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn aligns_columns_and_row_terminators_in_structured_environments() {
        let source = concat!(
            "\\begin{align*}\n",
            "a &= x + 1 \\\\ % first\n",
            "long_name&=y\\\\[1ex]\n",
            "z&=\\text{a & b}\n",
            "\\end{align*}\n",
            "\\begin{tabular}{ll}\n",
            "A & short\\\\\n",
            "Long & x \\\\\n",
            "AT\\&T & value\n",
            "\\end{tabular}\n"
        );
        let expected = concat!(
            "\\begin{align*}\n",
            "  a         & =x+1          \\\\ % first\n",
            "  long_name & =y            \\\\[1ex]\n",
            "  z         & =\\text{a & b}\n",
            "\\end{align*}\n",
            "\\begin{tabular}{ll}\n",
            "  A     & short \\\\\n",
            "  Long  & x     \\\\\n",
            "  AT\\&T & value\n",
            "\\end{tabular}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn preserves_alignment_padding_before_leading_empty_cells() {
        let source = concat!(
            "      \\begin{array}{ccc}\n",
            "        I& &A\\\\\n",
            "         &I&B\\\\\n",
            "         & &C\n",
            "      \\end{array}\n"
        );
        let expected = concat!(
            "\\begin{array}{ccc}\n",
            "  I &   & A \\\\\n",
            "    & I & B \\\\\n",
            "    &   & C\n",
            "\\end{array}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn can_disable_environment_row_alignment() {
        let source = "\\begin{align*}\na &= x \\\\\nlong&=y \\\\\n\\end{align*}\n";
        let options = WriterOptions {
            align_environment_rows: false,
            ..WriterOptions::default()
        };

        assert_eq!(
            write(source, &options),
            "\\begin{align*}\n  a&=x\\\\\n  long&=y\\\\\n\\end{align*}\n"
        );
    }

    #[test]
    fn preserves_verbatim_content() {
        let source = concat!(
            "\\begin{document}\n",
            "\\begin{verbatim}\n",
            "  literal   text   \n",
            "\n",
            "\n",
            "\\end{verbatim}\n",
            "\\end{document}\n"
        );
        let expected = concat!(
            "\\begin{document}\n",
            "\\begin{verbatim}\n",
            "  literal   text   \n",
            "\n",
            "\n",
            "\\end{verbatim}\n",
            "\\end{document}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn converts_line_endings() {
        let options = WriterOptions {
            line_ending: LineEnding::Crlf,
            ..WriterOptions::default()
        };

        assert_eq!(write("one\ntwo\n", &options), "one\r\ntwo\r\n");
    }

    #[test]
    fn retains_comment_contents() {
        let source = "% trailing spaces are comment text   \ntext   \n";
        let expected = "% trailing spaces are comment text   \ntext\n";

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn skips_formatting_between_comment_directives() {
        let source = concat!(
            "\\begin{document}\n",
            " before. After.   \n",
            "  % latex-treefmt: off  \r\n",
            "  $ x  + y $   \r\n",
            "\r\n",
            "  \\section{Untouched}   \r\n",
            "  % latex-treefmt: on  \r\n",
            " after. Again.   \n",
            "\\end{document}\n"
        );
        let expected = concat!(
            "\\begin{document}\n",
            "before.\n",
            "After.\n",
            "  % latex-treefmt: off  \r\n",
            "  $ x  + y $   \r\n",
            "\r\n",
            "  \\section{Untouched}   \r\n",
            "  % latex-treefmt: on  \r\n",
            "after.\n",
            "Again.\n",
            "\\end{document}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn skips_alignment_and_item_rewrites_inside_directives() {
        let source = concat!(
            "\\begin{align*}\n",
            "a &= x \\\\\n",
            "% latex-treefmt: off\n",
            "  long &= x + y   \\\\  \n",
            "% latex-treefmt: on\n",
            "b &= z \\\\\n",
            "\\end{align*}\n",
            "\\begin{enumerate}\n",
            "% latex-treefmt: off\n",
            "\\item First item. Second item.\n",
            "% latex-treefmt: on\n",
            "\\item Third item.\n",
            "\\end{enumerate}\n"
        );
        let output = write(source, &WriterOptions::default());

        assert!(output.contains("  long &= x + y   \\\\  \n"));
        assert!(output.contains("\\item First item. Second item.\n"));
        assert!(output.contains("  \\item\n    Third item.\n"));
        assert_eq!(write(&output, &WriterOptions::default()), output);
    }

    #[test]
    fn skips_to_end_of_file_without_an_on_directive() {
        let source = "before. After.\n% latex-treefmt: off\r\n  $ x + y $   ";
        let expected = "before.\nAfter.\n% latex-treefmt: off\r\n  $ x + y $   ";

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn skipped_lines_keep_their_line_endings_when_conversion_is_requested() {
        let source = "before\n% latex-treefmt: off\n  literal   \n% latex-treefmt: on\nafter\n";
        let options = WriterOptions {
            line_ending: LineEnding::Crlf,
            ..WriterOptions::default()
        };
        let expected =
            "before\r\n% latex-treefmt: off\n  literal   \n% latex-treefmt: on\nafter\r\n";

        assert_eq!(write(source, &options), expected);
    }

    #[test]
    fn preserves_multiple_regions_and_display_math_layout() {
        let source = concat!(
            "% latex-treefmt: off\n",
            "\\[ x + y \\]\n",
            "% latex-treefmt: on\n",
            "Between. Sentences.\n",
            "% latex-treefmt: off\n",
            "\\begin{equation} a + b \\end{equation}\n",
            "% latex-treefmt: on"
        );
        let expected = concat!(
            "% latex-treefmt: off\n",
            "\\[ x + y \\]\n",
            "% latex-treefmt: on\n",
            "Between.\n",
            "Sentences.\n",
            "% latex-treefmt: off\n",
            "\\begin{equation} a + b \\end{equation}\n",
            "% latex-treefmt: on"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn ignores_directive_text_in_verbatim_and_inline_comments() {
        let source = concat!(
            "\\begin{verbatim}\n",
            "% latex-treefmt: off\n",
            "\\end{verbatim}\n",
            "before. After. % latex-treefmt: off\n",
            "following. Sentence.\n"
        );
        let output = write(source, &WriterOptions::default());

        assert!(output.contains("before.\nAfter. % latex-treefmt: off\n"));
        assert!(output.contains("following.\nSentence.\n"));
    }

    #[test]
    fn ignores_on_directive_inside_verbatim_within_skipped_region() {
        let source = concat!(
            "% latex-treefmt: off\n",
            "\\begin{verbatim}\n",
            "% latex-treefmt: on\n",
            "\\end{verbatim}\n",
            "  $ x + y $   \n",
            "% latex-treefmt: on\n",
            "After. Another.\n"
        );
        let expected = source.replace("After. Another.\n", "After.\nAnother.\n");

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn compacts_math_without_changing_control_words_or_text() {
        let source = concat!(
            "$ \\forall   c \\in \\text{two words} \\quad L^{U} + N_{1} $\n",
            "$ \\operatorname{special linear} ( x ) + e^{2\\pi i/3} $\n",
            "$a\\ b$\n"
        );
        let expected = concat!(
            "$\\forall c\\in\\text{two words}\\quad L^U+N_1$\n",
            "$\\operatorname{special linear}(x)+e^{2\\pi i/3}$\n",
            "$a\\ b$\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn keeps_script_braces_when_a_control_word_precedes_a_letter() {
        let source = "$T^{\\top}x+A_{\\alpha} y+B^{\\top}+C$\n";
        let expected = "$T^{\\top}x+A_{\\alpha}y+B^\\top+C$\n";

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn wraps_sentences_and_separates_sections() {
        let source = concat!(
            "First sentence. Second sentence with cf. Section A.\n",
            "\\subsection{Next}\n",
            "Body text. Another sentence.\n"
        );
        let expected = concat!(
            "First sentence.\n",
            "Second sentence with cf. Section A.\n",
            "\n",
            "\\subsection{Next}\n",
            "\n",
            "Body text.\n",
            "Another sentence.\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn moves_prose_after_a_section_heading_to_its_own_paragraph() {
        let source = concat!(
            "Before.\n",
            "\\section{Heisenberg picture of CNOT gates} ",
            "A two qubit unitary acts on the target. Another sentence.\n"
        );
        let expected = concat!(
            "Before.\n",
            "\n",
            "\\section{Heisenberg picture of CNOT gates}\n",
            "\n",
            "A two qubit unitary acts on the target.\n",
            "Another sentence.\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn separates_optional_and_nested_section_titles_from_following_text() {
        let source = "\\subsection[Short]{A \\textbf{long} title} Body. Next.\n";
        let expected = "\\subsection[Short]{A \\textbf{long} title}\n\nBody.\nNext.\n";
        assert_eq!(write(source, &WriterOptions::default()), expected);

        let options = WriterOptions {
            blank_lines_around_sections: false,
            ..WriterOptions::default()
        };
        let without_blank = "\\subsection[Short]{A \\textbf{long} title}\nBody.\nNext.\n";
        assert_eq!(write(source, &options), without_blank);
    }

    #[test]
    fn keeps_trailing_section_comments_on_the_heading_line() {
        let source = "\\section{Title} % keep this comment\nBody text.\n";
        let expected = "\\section{Title} % keep this comment\n\nBody text.\n";
        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn keeps_a_section_label_on_the_heading_line() {
        let source = "\\section{Introduction}\n\n\\label{sec:intro}\n\nBody text.\n";
        let expected = "\\section{Introduction}\\label{sec:intro}\n\nBody text.\n";

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn keeps_an_inline_section_label_before_following_prose() {
        let source = "\\section{Title}\\label{sec:title} Body. Next.\n";
        let expected = "\\section{Title}\\label{sec:title}\n\nBody.\nNext.\n";

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn does_not_move_a_section_label_out_of_a_skipped_region() {
        let source = concat!(
            "\\section{Title}\n",
            "% latex-treefmt: off\n",
            "\\label{sec:title}\n",
            "% latex-treefmt: on\n",
            "Body text.\n"
        );
        let output = write(source, &WriterOptions::default());

        assert!(output.contains("\\section{Title}\n% latex-treefmt: off\n"));
        assert!(output.contains("\\label{sec:title}\n% latex-treefmt: on\n"));
    }

    #[test]
    fn wraps_indented_sentences_inside_prose_environments() {
        let source = concat!(
            "\\begin{proof}\n",
            "  First sentence. Second sentence with $A(F)$. Third sentence.\n",
            "\\end{proof}\n",
            "\\begin{abstract}\n",
            "  Another sentence. Final sentence.\n",
            "\\end{abstract}\n"
        );
        let expected = concat!(
            "\\begin{proof}\n",
            "  First sentence.\n",
            "  Second sentence with $A(F)$.\n",
            "  Third sentence.\n",
            "\\end{proof}\n",
            "\\begin{abstract}\n",
            "  Another sentence.\n",
            "  Final sentence.\n",
            "\\end{abstract}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn keeps_prose_environment_bodies_adjacent_to_their_boundaries() {
        let source = concat!(
            "\\begin{definition}[Hypergraph product code] ",
            "First sentence. Second sentence.\n",
            "\n",
            "\\end{definition}\n"
        );
        let expected = concat!(
            "\\begin{definition}[Hypergraph product code]\n",
            "  First sentence.\n",
            "  Second sentence.\n",
            "\\end{definition}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn keeps_an_environment_label_with_its_opening_command() {
        let source = concat!(
            "\\begin{theorem}[Named]\n",
            "\n",
            "  \\label{thm:named}\n",
            "\n",
            "Statement. More text. \\end{theorem}\n"
        );
        let expected = concat!(
            "\\begin{theorem}[Named]\\label{thm:named}\n",
            "  Statement.\n",
            "  More text.\n",
            "\\end{theorem}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn canonicalizes_environment_headers_boundaries_and_nested_content() {
        let source = concat!(
            "Before \\begin{foo} [optional]\n",
            "  \\label{env:foo} {required}\n",
            "\n",
            "  First sentence. Second sentence. \\begin{bar} inner \\end{bar}\n",
            "\n",
            "\\end{foo} After\n"
        );
        let expected = concat!(
            "Before\n",
            "\\begin{foo}[optional]{required}\\label{env:foo}\n",
            "  First sentence.\n",
            "  Second sentence.\n",
            "  \\begin{bar}\n",
            "    inner\n",
            "  \\end{bar}\n",
            "\\end{foo}\n",
            "After\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn preserves_raw_environment_contents_during_environment_normalization() {
        let source = concat!(
            "\\begin{verbatim}\n",
            "\\begin{foo} inline \\end{foo}\n",
            "\n",
            "\\end{verbatim}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), source);
    }

    #[test]
    fn wraps_a_sentence_starting_with_inline_math() {
        let source = concat!(
            "For $x\\in\\F_2^n$ let $\\wt{x}$ denote its Hamming weight.  ",
            "$\\cnot_{ij}$ denotes a controlled-NOT operation.\n"
        );
        let expected = concat!(
            "For $x\\in\\F_2^n$ let $\\wt{x}$ denote its Hamming weight.\n",
            "$\\cnot_{ij}$ denotes a controlled-NOT operation.\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn wraps_a_sentence_starting_with_parenthesized_math() {
        let source = "First sentence. \\(x\\) starts the next one.\n";
        let options = WriterOptions {
            inline_math_delimiters: InlineMathDelimiters::Preserve,
            ..WriterOptions::default()
        };
        let expected = "First sentence.\n\\(x\\) starts the next one.\n";

        assert_eq!(write(source, &options), expected);
    }

    #[test]
    fn preserves_existing_prose_indentation_when_indent_is_disabled() {
        let source = "\\begin{proof}\n\tFirst sentence. Second sentence.\n\n\t\\end{proof}\n";
        let options = WriterOptions {
            indent_environments: false,
            ..WriterOptions::default()
        };
        let expected = "\\begin{proof}\n\tFirst sentence.\n\tSecond sentence.\n\t\\end{proof}\n";

        assert_eq!(write(source, &options), expected);
    }

    #[test]
    fn does_not_split_sentence_like_text_in_structured_environments() {
        let source = concat!(
            "\\begin{tabular}{l}\n",
            "First. Second. \\\\\n",
            "\\end{tabular}\n",
            "\\begin{equation}\n",
            "A. B\n",
            "\\end{equation}\n"
        );
        let output = write(source, &WriterOptions::default());

        assert!(output.contains("First. Second."));
        assert!(output.contains("A.B"));
    }

    #[test]
    fn sentence_wrapping_avoids_comments_groups_and_verbatim() {
        let source = concat!(
            "Text % comment. Next\n",
            "\\begin{verbatim}\n",
            "  Sentence. Next.\n",
            "\\section{literal}\n",
            "\\end{verbatim}\n",
            "A \\textit{Not. Split} ending. Next sentence.\n"
        );
        let expected = concat!(
            "Text % comment. Next\n",
            "\\begin{verbatim}\n",
            "  Sentence. Next.\n",
            "\\section{literal}\n",
            "\\end{verbatim}\n",
            "A \\textit{Not. Split} ending.\n",
            "Next sentence.\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn collapses_repeated_spaces_in_prose_after_inline_math() {
        let source = concat!(
            "\\begin{theorem}\n",
            "  $\\ker(H_1^T)$     and take $e_1^T$ to satisfy the condition.\n",
            "\\end{theorem}\n"
        );
        let expected = concat!(
            "\\begin{theorem}\n",
            "  $\\ker(H_1^T)$ and take $e_1^T$ to satisfy the condition.\n",
            "\\end{theorem}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
        assert_eq!(write(expected, &WriterOptions::default()), expected);
    }

    #[test]
    fn preserves_protected_spaces_while_collapsing_prose() {
        let source = "Before  $x$    and \\textbf{two  words}  after. % comment  kept\n";
        let expected = "Before $x$ and \\textbf{two  words} after. % comment  kept\n";
        assert_eq!(write(source, &WriterOptions::default()), expected);

        let options = WriterOptions {
            collapse_prose_whitespace: false,
            ..WriterOptions::default()
        };
        assert_eq!(write(source, &options), source);
    }

    #[test]
    fn separates_math_environments_from_adjacent_prose() {
        let source = concat!(
            "Before \\begin{equation*}\n",
            "x + y\n",
            "\\end{equation*}After\n"
        );
        let expected = concat!(
            "Before\n",
            "\\begin{equation*}\n",
            "  x+y\n",
            "\\end{equation*}\n",
            "After\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn converts_paired_command_math_delimiters_to_dollars() {
        let source = concat!(
            "Inline \\( x + 1 \\).\n",
            "\\[\n",
            "y^{2}\n",
            "\\]\n",
            "Mixed \\(z$.\n"
        );
        let expected = concat!("Inline $x+1$.\n", "$$\n", "  y^2\n", "$$\n", "Mixed $z$.\n");

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn renders_user_selected_math_delimiters() {
        let source = "Dollar $ x + 1 $ and display $$ y^{2} $$.\n";
        let options = WriterOptions {
            inline_math_delimiters: InlineMathDelimiters::Parentheses,
            display_math_delimiters: DisplayMathDelimiters::Brackets,
            ..WriterOptions::default()
        };

        assert_eq!(
            write(source, &options),
            "Dollar \\(x+1\\) and display \\[y^2\\].\n"
        );
    }

    #[test]
    fn display_math_layout_is_adaptive_by_default() {
        let source = concat!(
            "Inline display $$ x + 1 $$ stays here.\n",
            "Prefix \\[ x + y\n",
            "z \\] suffix\n"
        );
        let expected = concat!(
            "Inline display $$x+1$$ stays here.\n",
            "Prefix\n",
            "$$\n",
            "  x+y\n",
            "  z\n",
            "$$\n",
            "suffix\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn can_render_every_display_formula_as_a_block() {
        let source = "Before $$ x + 1 $$ after.\n";
        let options = WriterOptions {
            display_math_layout: DisplayMathLayout::Block,
            ..WriterOptions::default()
        };

        assert_eq!(write(source, &options), "Before\n$$\n  x+1\n$$\nafter.\n");
    }

    #[test]
    fn can_preserve_math_delimiter_styles_while_formatting_contents() {
        let source = "Command \\( x + 1 \\) and dollar $ y + 2 $.\n";
        let options = WriterOptions {
            inline_math_delimiters: InlineMathDelimiters::Preserve,
            ..WriterOptions::default()
        };

        assert_eq!(
            write(source, &options),
            "Command \\(x+1\\) and dollar $y+2$.\n"
        );
    }

    #[test]
    fn indents_display_blocks_relative_to_their_environment() {
        let source = concat!(
            "\\begin{itemize}\n",
            "\\[\n",
            "x + y\n",
            "\\]\n",
            "\\end{itemize}\n"
        );
        let expected = concat!(
            "\\begin{itemize}\n",
            "  $$\n",
            "    x+y\n",
            "  $$\n",
            "\\end{itemize}\n"
        );

        assert_eq!(write(source, &WriterOptions::default()), expected);
    }

    #[test]
    fn can_preserve_display_math_line_placement() {
        let source = "\\[\nx + y\n\\]\n";
        let options = WriterOptions {
            display_math_layout: DisplayMathLayout::Preserve,
            ..WriterOptions::default()
        };

        assert_eq!(write(source, &options), "$$\nx+y\n$$\n");
    }
}
