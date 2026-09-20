# latex-treefmt

A configurable LaTeX formatter built around a loss-aware syntax tree, including
structured math expressions.

The first working implementation uses `tree-sitter-latex` 0.6.0 to construct a
concrete syntax tree, then renders from that tree through a source-backed writer.
The parser evaluation is documented in
[`docs/tree-sitter-latex-evaluation.md`](docs/tree-sitter-latex-evaluation.md).

## Intended pipeline

```text
LaTeX source
  -> resilient concrete syntax tree
  -> loss-aware document AST + math-expression AST
  -> configurable layout IR
  -> rendered LaTeX
```

Unknown or unsupported constructs must remain representable as raw, source-backed
nodes so formatting never silently drops input.

The current writer formats:

- environment indentation, with `document` contents kept at the outer level by
  default;
- aligned `&` column separators and `\\` row terminators in table and alignment
  environments;
- compact math spacing while retaining lexically required control-word spaces;
- configurable inline and display math delimiters, defaulting to `$...$` and
  `$$...$$`;
- adaptive display-math layout: same-line delimiters stay inline, while
  multiline displays use delimiter-only lines and indented contents;
- `latexindent`-style item layout, with each `\item` and its body starting on
  separate lines;
- redundant braces around single-token subscripts and superscripts;
- recognized math-environment boundaries adjacent to prose;
- one prose sentence per source line;
- blank lines around sectioning commands;
- trailing whitespace, repeated blank lines, line endings, and the final newline.

Text-bearing math commands such as `\text{...}` and `\operatorname{...}`, line
comments, and verbatim-like environment contents are protected from destructive
whitespace rewriting. Unknown syntax remains available through preserve mode.

## Usage

Format a file to standard output:

```sh
cargo run -- document.tex
```

After cloning, initialize the grammar and generate its ignored parser source
once before using Cargo:

```sh
git submodule update --init
./scripts/generate-tree-sitter-latex.sh
```

Read standard input and use four-space environment indentation:

```sh
cargo run -- --indent-width 4 < document.tex
```

Reconstruct the complete CST byte-for-byte, including anonymous tokens and
whitespace between nodes:

```sh
cargo run -- --preserve document.tex
```

Run `cargo run -- --help` for all writer options. Formatting recovered trees is
allowed, but the CLI emits a warning when Tree-sitter reports parse errors.
Each structural rule has a corresponding opt-out flag.

Choose command-style delimiters independently for inline and display math:

```sh
cargo run -- \
  --inline-math-delimiters parentheses \
  --display-math-delimiters brackets \
  document.tex
```

Each delimiter option also accepts `dollars` or `preserve`.

Force every displayed formula into the multiline environment-like layout:

```sh
cargo run -- --display-math-layout block document.tex
```

The other display-layout modes are `adaptive` (the default) and `preserve`.

Pass `--indent-document` to include the `document` environment in normal
environment indentation.

Pass `--no-align-environments` to retain the original placement of `&` column
separators and `\\` row terminators.

Pass `--no-item-line-breaks` to preserve existing line breaks around `\item`
declarations instead of applying the default `ItemStartsOnOwnLine: 1` and
`ItemFinishesWithLineBreak: 1` behavior.

### Skip a region

Put these comments on their own lines to leave a region exactly as written:

```tex
This sentence. Gets formatted.
% latex-treefmt: off
  $ x  + y $   % spacing kept
% latex-treefmt: on
This sentence. Gets formatted too.
```

The directive lines are preserved as well. Leading and trailing spaces on those
lines are allowed. If an `off` directive has no matching `on` directive, the
rest of the file is left unchanged. Directives inside verbatim-like environments
or at the end of another line are treated as ordinary text.

## Use it in Neovim

Install the formatter with `cargo install --path .` and register it as a
stdin/stdout formatting source. For `none-ls.nvim`, add this to your `sources`
list:

```lua
{
  name = "latex-treefmt",
  method = null_ls.methods.FORMATTING,
  filetypes = { "tex" },
  generator = null_ls.formatter({
    command = "latex-treefmt",
    to_stdin = true,
  }),
},
```

Run `cargo install --path . --force` after rebuilding this project so Neovim
uses the latest binary. Make sure Cargo's bin directory is on Neovim's `PATH`.
Use `:echo executable('latex-treefmt')` to check that Neovim can find it, then
`:NullLsInfo` to confirm that it is registered for a TeX buffer.

## Parser dependency

The grammar repository is pinned to upstream v0.6.0 as the
[`vendor/tree-sitter-latex`](vendor/tree-sitter-latex) Git submodule, and Cargo
uses that crate directly. The upstream tag omits the ignored `src/parser.c`, so
[`scripts/generate-tree-sitter-latex.sh`](scripts/generate-tree-sitter-latex.sh)
generates its ABI-14 parser and headers locally from the pinned grammar. No
generated parser artifact is tracked by this repository.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Rust is used for the initial scaffold because Tree-sitter has native Rust
bindings and the formatter core can later be embedded in a CLI, editor, or WASM
host.
