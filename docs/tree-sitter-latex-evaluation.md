# Is `tree-sitter-latex` sufficient?

Status: **conditional go for the document CST; no-go as the complete formatter parser**.

Adoption update: the project now uses a pinned v0.6.0 grammar as its initial tree constructor. Its nodes remain behind project-owned parser and writer APIs, so a future adapter or parser can be introduced without changing callers.

Evaluated on 2026-09-18 against upstream `latex-lsp/tree-sitter-latex` 0.6.0.
The probe corpus is in
[`tests/fixtures/tree-sitter-latex-probe.input.tex`](../tests/fixtures/tree-sitter-latex-probe.input.tex).

## Decision

Use `tree-sitter-latex` as the initial error-tolerant concrete syntax parser. Do not expose its node tree as the formatter's public AST, and do not rely on it to parse mathematical expressions.

Put a parser adapter between Tree-sitter and a formatter-owned, loss-aware AST. Run a dedicated math parser over the source ranges identified as inline or display math. Preserve unsupported constructs as raw nodes with exact source ranges.

The initial writer is source-backed and has a byte-for-byte preserve mode. This establishes the lossless baseline before richer document and math AST adapters are introduced.

## What the grammar does well

| Requirement                   | Result        | Notes                                                                                                        |
| ----------------------------- | ------------- | ------------------------------------------------------------------------------------------------------------ |
| Error-tolerant parsing        | Good          | Tree-sitter produces a tree for incomplete input and marks recovery nodes.                                   |
| Document structure            | Good baseline | Sections, groups, many common commands, environments, includes, citations, and definitions have named nodes. |
| Math-region detection         | Good baseline | `$...$`, `\(...\)`, `$$...$$`, `\[...\]`, and a fixed list of common math environments are recognized.       |
| Editor/incremental use        | Good          | Byte ranges and incremental reparsing are a natural fit for future editor integration.                       |
| Comments and source locations | Useful        | Line comments are nodes and all parsed nodes carry byte ranges. Exact source slices can be retained.         |
| Verbatim-like content         | Partial       | Several common environments have external-scanner support, but the list is necessarily finite.               |

## Probe observations

Parsing the checked-in probe with the upstream grammar produced no recovery errors, but showed the structural gaps directly:

- `a + b * c` became a flat sequence of words and operator tokens.
- `\frac{a+b}{c}` became a `generic_command` with two curly groups.
- `\sqrt[3]{x}` did not associate the bracketed index with the command.
- `custommath` became a `generic_environment`, so its body was parsed as text.
- mismatched `\begin{alpha}` and `\end{beta}` names were accepted in one `generic_environment` node.

These are valid choices for syntax highlighting and navigation, but they leave semantic work that the formatter cannot safely infer in its printer.

## Why it is not sufficient by itself

### Math is a region, not an expression tree

The grammar's `inline_formula` and `displayed_equation` rules repeat the general root-content rule. Inside them, `+`, `-`, `*`, `/`, and similar characters are flat `operator` tokens. There are nodes for subscript, superscript, and paired delimiters, but no expression precedence, application, fraction, relation, matrix-cell, or alignment-row model. For example, `\frac{a+b}{c}` is a generic command with groups; `a+b*c` has no multiplication subtree.

A customizable math formatter needs a formatter-owned math AST such as lists, atoms, scripts, fractions, delimiters, rows/cells, and explicit fallback nodes.

### LaTeX command syntax is open-ended

The generic command rule accepts a command name followed by zero or more curly groups. Packages and user definitions can introduce optional, delimited, starred, verbatim, and otherwise context-sensitive arguments. The grammar has special rules for many known commands, but cannot infer arbitrary signatures from package or macro definitions.

The formatter therefore needs a configurable command/environment signature registry. It should recognize a conservative subset and preserve anything else without guessing.

### Environment semantics are incomplete

Known math environments are enumerated in the grammar. A package-defined math environment falls back to a generic environment and is not known to contain math. Generic `begin` and `end` names are parsed independently, so semantic name matching belongs in the adapter/validation layer.

### Whitespace is intentionally not an AST node

Whitespace is a Tree-sitter `extra`. That is fine for deliberate reformatting, but it means lossless behavior must come from source ranges and retained raw slices rather than from walking named nodes alone. This is important for disabled regions and unsupported syntax.

### Full TeX semantics are out of scope

Category-code changes and macro expansion can change tokenization and meaning. The upstream project explicitly describes its parsing as best-effort and focused on LaTeX-level constructs rather than TeX internals. The formatter should make the same boundary explicit instead of promising full TeX interpretation.

## Recommended architecture

```text
source text
  -> tree-sitter-latex CST (regions, recovery, byte spans)
  -> adapter/validator
       - attach comments and preserve raw slices
       - validate matching environments
       - consult command/environment signature configuration
  -> formatter-owned document AST
       - invoke dedicated math parser for math ranges
       - retain Raw nodes for unsupported constructs
  -> layout IR (groups, indentation, soft/hard line breaks)
  -> configurable printer
```

The document parser and math parser should be traits/interfaces owned by this project. That prevents the rendering layer and public configuration model from depending on upstream node names.

## Dependency and maintenance notes

- Upstream is active and versioned as 0.6.0, with Rust, Node, Python, Go, Swift, and C bindings in its repository.
- The official README says the grammar originated from TexLab and targets language-server-relevant constructs.
- At evaluation time, the upstream repository and the readily discoverable crates.io package were not aligned: docs.rs showed `tree-sitter-latex` 0.1.0 under a different crates.io owner, while upstream declared 0.6.0 and had an open issue about publishing the parser to crates.io. The project pins the official repository as a Git submodule at v0.6.0 and generates the tag's ignored `src/parser.c` locally with ABI 14 before building.
- The grammar is MIT-licensed.

## Corpus gate for expanding formatter behavior

Before adding structural token or math rewriting, require:

1. Byte-for-byte reconstruction from retained source slices for every corpus case before formatting is enabled.
2. No dropped comments or content, including malformed input.
3. Correct math-region boundaries for standard LaTeX and AMS math delimiters.
4. Correct fallback behavior for custom commands, custom environments, `verbatim`-like content, and category-code-changing input.
5. A separate math AST that distinguishes at least groups, scripts, fractions, delimiters, relations, binary operators, and aligned rows/cells.
6. Version-pinned parser generation that is reproducible in CI.

## Sources

- [Upstream README and stated limitations](https://github.com/latex-lsp/tree-sitter-latex)
- [Upstream grammar](https://github.com/latex-lsp/tree-sitter-latex/blob/master/grammar.js)
- [Upstream 0.6.0 metadata](https://github.com/latex-lsp/tree-sitter-latex/blob/master/tree-sitter.json)
- [Upstream releases](https://github.com/latex-lsp/tree-sitter-latex/releases)
- [Rust package documentation currently discoverable as 0.1.0](https://docs.rs/tree-sitter-latex/latest/tree_sitter_latex/)
- [Tree-sitter grammar design documentation](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html)
- [Upstream issues, including crates.io publishing](https://github.com/latex-lsp/tree-sitter-latex/issues)
