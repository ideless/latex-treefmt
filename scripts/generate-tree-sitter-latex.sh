#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
grammar_dir="$project_root/vendor/tree-sitter-latex"
generated_dir=$(mktemp -d "${TMPDIR:-/tmp}/tree-sitter-latex.XXXXXX")

cleanup() {
    rm -rf -- "$generated_dir"
}
trap cleanup EXIT HUP INT TERM

if ! command -v tree-sitter >/dev/null 2>&1; then
    echo "tree-sitter CLI is required; install tree-sitter-cli 0.24.1 or newer" >&2
    exit 1
fi

if [ ! -f "$grammar_dir/grammar.js" ]; then
    echo "tree-sitter-latex submodule is missing; run git submodule update --init" >&2
    exit 1
fi

(
    cd "$grammar_dir"
    tree-sitter generate --abi 14 -o "$generated_dir"
)

mkdir -p "$grammar_dir/src/tree_sitter"
cp "$generated_dir/parser.c" "$grammar_dir/src/parser.c"
cp "$generated_dir/tree_sitter/"*.h "$grammar_dir/src/tree_sitter/"
