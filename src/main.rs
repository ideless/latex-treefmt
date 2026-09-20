use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::process::ExitCode;

use latex_treefmt::{
    DisplayMathDelimiters, DisplayMathLayout, InlineMathDelimiters, LineEnding, WriterOptions,
    format,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("latex-treefmt: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (path, options) = parse_args()?;
    let source = read_source(path.as_deref())?;
    let result = format(&source, &options).map_err(|error| error.to_string())?;

    if result.had_parse_errors {
        eprintln!("latex-treefmt: warning: Tree-sitter recovered from parse errors");
    }

    io::stdout()
        .write_all(result.text.as_bytes())
        .map_err(|error| format!("could not write stdout: {error}"))
}

fn parse_args() -> Result<(Option<String>, WriterOptions), String> {
    let mut options = WriterOptions::default();
    let mut path = None;
    let mut arguments = env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--preserve" => options = WriterOptions::preserve(),
            "--no-indent" => options.indent_environments = false,
            "--indent-document" => options.indent_document = true,
            "--keep-trailing-whitespace" => options.trim_trailing_whitespace = false,
            "--no-final-newline" => options.ensure_final_newline = false,
            "--keep-math-whitespace" => options.compact_math = false,
            "--keep-script-braces" => options.simplify_math_scripts = false,
            "--no-sentence-wrap" => options.sentence_per_line = false,
            "--no-section-blank-lines" => options.blank_lines_around_sections = false,
            "--allow-inline-math-environments" => {
                options.separate_math_environment_boundaries = false;
            }
            "--no-align-environments" => options.align_environment_rows = false,
            "--no-item-line-breaks" => {
                options.item_starts_on_own_line = false;
                options.item_finishes_with_line_break = false;
            }
            "--inline-math-delimiters" => {
                let value = arguments.next().ok_or_else(|| {
                    "--inline-math-delimiters requires preserve, dollars, or parentheses".to_owned()
                })?;
                options.inline_math_delimiters = match value.as_str() {
                    "preserve" => InlineMathDelimiters::Preserve,
                    "dollars" => InlineMathDelimiters::Dollars,
                    "parentheses" => InlineMathDelimiters::Parentheses,
                    _ => return Err(format!("invalid inline math delimiters: {value}")),
                };
            }
            "--display-math-delimiters" => {
                let value = arguments.next().ok_or_else(|| {
                    "--display-math-delimiters requires preserve, dollars, or brackets".to_owned()
                })?;
                options.display_math_delimiters = match value.as_str() {
                    "preserve" => DisplayMathDelimiters::Preserve,
                    "dollars" => DisplayMathDelimiters::Dollars,
                    "brackets" => DisplayMathDelimiters::Brackets,
                    _ => return Err(format!("invalid display math delimiters: {value}")),
                };
            }
            "--display-math-layout" => {
                let value = arguments.next().ok_or_else(|| {
                    "--display-math-layout requires preserve, adaptive, or block".to_owned()
                })?;
                options.display_math_layout = match value.as_str() {
                    "preserve" => DisplayMathLayout::Preserve,
                    "adaptive" => DisplayMathLayout::Adaptive,
                    "block" => DisplayMathLayout::Block,
                    _ => return Err(format!("invalid display math layout: {value}")),
                };
            }
            "--indent-width" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--indent-width requires a number".to_owned())?;
                options.indent_width = value
                    .parse()
                    .map_err(|_| format!("invalid indent width: {value}"))?;
            }
            "--max-blank-lines" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--max-blank-lines requires a number".to_owned())?;
                options.max_blank_lines = Some(
                    value
                        .parse()
                        .map_err(|_| format!("invalid blank-line limit: {value}"))?,
                );
            }
            "--line-ending" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--line-ending requires preserve, lf, or crlf".to_owned())?;
                options.line_ending = match value.as_str() {
                    "preserve" => LineEnding::Preserve,
                    "lf" => LineEnding::Lf,
                    "crlf" => LineEnding::Crlf,
                    _ => return Err(format!("invalid line ending: {value}")),
                };
            }
            "-" => {
                if path.replace(argument).is_some() {
                    return Err("only one input file is supported".to_owned());
                }
            }
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            _ => {
                if path.replace(argument).is_some() {
                    return Err("only one input file is supported".to_owned());
                }
            }
        }
    }

    Ok((path, options))
}

fn read_source(path: Option<&str>) -> Result<String, String> {
    match path {
        Some("-") | None => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|error| format!("could not read stdin: {error}"))?;
            Ok(source)
        }
        Some(path) => {
            fs::read_to_string(path).map_err(|error| format!("could not read {path}: {error}"))
        }
    }
}

fn print_help() {
    println!(
        "latex-treefmt [OPTIONS] [FILE]\n\
         \n\
         Reads FILE, or stdin when FILE is omitted or '-'. Writes formatted LaTeX to stdout.\n\
         \n\
         Options:\n\
           --preserve                  Reconstruct the source byte-for-byte\n\
           --indent-width N            Spaces per environment level (default: 2)\n\
           --no-indent                 Keep existing leading indentation\n\
           --indent-document           Indent content inside the document environment\n\
           --keep-trailing-whitespace  Keep spaces and tabs at line ends\n\
           --max-blank-lines N         Maximum consecutive blank lines (default: 1)\n\
           --line-ending MODE          preserve, lf, or crlf\n\
           --no-final-newline          Do not add a missing final newline\n\
           --keep-math-whitespace      Keep horizontal whitespace inside math\n\
           --keep-script-braces        Keep braces around single-token scripts\n\
           --no-sentence-wrap          Do not put prose sentences on separate lines\n\
           --no-section-blank-lines    Do not add blank lines around sections\n\
           --allow-inline-math-environments\n\
                                       Do not isolate math-environment boundaries\n\
           --no-align-environments     Do not align & and \\\\ in table/alignment environments\n\
           --no-item-line-breaks       Do not add line breaks around \\item declarations\n\
           --inline-math-delimiters MODE\n\
                                       preserve, dollars (default), or parentheses\n\
           --display-math-delimiters MODE\n\
                                       preserve, dollars (default), or brackets\n\
           --display-math-layout MODE  preserve, adaptive (default), or block\n\
           -h, --help                  Show this help"
    );
}
