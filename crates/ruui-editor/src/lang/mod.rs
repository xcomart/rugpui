//! One [`Highlighter`] per language this crate ships a lexer for, the registry
//! that lists them, and the rules that pick one for a file.
//!
//! A host with a path and nothing else asks
//! [`highlighter_for_extension`]. A host that has to answer "what is this file"
//! for a name with no extension — half the shell scripts on a server are called
//! `deploy` — or that offers a language picker, holds a
//! [`LanguageRegistry`](registry::LanguageRegistry) instead; that module
//! documents the three detection rules and their order. A host that wants a
//! second grammar painted over one of these — a template language, a
//! placeholder syntax — composes it on top with
//! [`CompositeHighlighter`](crate::composite::CompositeHighlighter); the base
//! is what this module is.
//!
//! # The three ways a language is implemented here
//!
//! **A lexer of its own**, for the languages whose grammar is not the "keyword,
//! string, line comment, block comment" shape the others share: the seven
//! configuration formats a file panel reaches every day —
//! [`shell`](shell::ShellHighlighter), [`yaml`](yaml::YamlHighlighter),
//! [`json`](json::JsonHighlighter), [`toml`](toml::TomlHighlighter),
//! [`conf`](conf::ConfHighlighter),
//! [`dockerfile`](dockerfile::DockerfileHighlighter) and
//! [`markdown`](markdown::MarkdownHighlighter) — plus
//! [`java`](java::JavaHighlighter) (annotations, text blocks),
//! [`xml`](xml::XmlHighlighter) (tags and attributes, also used for HTML) and
//! [`php`](php::PhpHighlighter) (`$variables`, a `<?php` boundary, and strings
//! that span line breaks). SQL is
//! [`SqlHighlighter`](crate::sql_syntax::SqlHighlighter), unchanged.
//!
//! **[`clike::CLikeHighlighter`] with a [`clike::CLikeConfig`]** of its own
//! keyword table and comment/string syntax, for the languages that do share
//! that shape: C#, Kotlin, TypeScript and JavaScript, Go, Rust and Python.
//!
//! **A definition read from a file**, behind the `custom-syntax` feature: see
//! [`mod@custom`]. Off by default, because it costs a YAML reader.
//!
//! # What the configuration lexers are, and are not
//!
//! Every one of them is a hand-written state machine over bytes, and none of
//! them builds a tree. What an editor needs is what a good `cat` would give
//! you: a comment is grey, a string is green, the left-hand side of a mapping
//! stands out from the right. A `.yml` that is invalid YAML still has to be
//! readable *while it is being fixed*, which is the argument against a real
//! parser as much as the size of one is. The rule each is held to is that it
//! never panics and never refuses — a line of random bytes comes out with no
//! spans at all, not as an error — and that whatever it carries to the next
//! line fits inside [`LineState::COMPOSABLE_BITS`](crate::LineState), so any of
//! them can be the base under an overlay. Each module documents its own
//! encoding.
//!
//! # Extension resolution
//!
//! [`highlighter_for_extension`] is case-insensitive and looks at the extension
//! alone, in the form [`std::path::Path::extension`] returns it — no leading
//! dot. An extension this crate has no lexer for answers `None`, which an
//! editor draws as plain text. A host whose files carry a second extension of
//! their own — `Model.java.tpl` — strips it before asking, so that the lookup
//! lands on the language the file is really written in.

pub mod clike;
pub mod conf;
#[cfg(feature = "custom-syntax")]
pub mod custom;
pub mod dockerfile;
pub mod java;
pub mod json;
pub mod markdown;
pub mod php;
pub mod registry;
mod scan;
pub mod shell;
pub mod toml;
pub mod xml;
pub mod yaml;

use std::sync::Arc;

use crate::highlight::Highlighter;
use crate::sql_syntax::SqlHighlighter;
use clike::{CLikeConfig, CLikeHighlighter};
use conf::ConfHighlighter;
use dockerfile::DockerfileHighlighter;
use java::JavaHighlighter;
use json::JsonHighlighter;
use markdown::MarkdownHighlighter;
use php::PhpHighlighter;
pub use registry::{FileMatch, LanguageEntry, LanguageRegistry};
use shell::ShellHighlighter;
use toml::TomlHighlighter;
use xml::XmlHighlighter;
use yaml::YamlHighlighter;

/// C#.
const CSHARP: CLikeConfig = CLikeConfig {
    keywords: &[
        "abstract",
        "as",
        "async",
        "await",
        "base",
        "bool",
        "break",
        "byte",
        "case",
        "catch",
        "char",
        "checked",
        "class",
        "const",
        "continue",
        "decimal",
        "default",
        "delegate",
        "do",
        "double",
        "dynamic",
        "else",
        "enum",
        "event",
        "explicit",
        "extern",
        "false",
        "finally",
        "fixed",
        "float",
        "for",
        "foreach",
        "get",
        "goto",
        "if",
        "implicit",
        "in",
        "init",
        "int",
        "interface",
        "internal",
        "is",
        "lock",
        "long",
        "namespace",
        "new",
        "null",
        "object",
        "operator",
        "out",
        "override",
        "params",
        "partial",
        "private",
        "protected",
        "public",
        "readonly",
        "record",
        "ref",
        "return",
        "sbyte",
        "sealed",
        "set",
        "short",
        "sizeof",
        "stackalloc",
        "static",
        "string",
        "struct",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "uint",
        "ulong",
        "unchecked",
        "unsafe",
        "ushort",
        "using",
        "var",
        "virtual",
        "void",
        "volatile",
        "while",
        "yield",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    triple_quotes: &[],
};

/// Kotlin.
const KOTLIN: CLikeConfig = CLikeConfig {
    keywords: &[
        "abstract",
        "actual",
        "annotation",
        "as",
        "break",
        "by",
        "catch",
        "class",
        "companion",
        "const",
        "constructor",
        "continue",
        "crossinline",
        "data",
        "do",
        "dynamic",
        "else",
        "enum",
        "expect",
        "external",
        "false",
        "final",
        "finally",
        "for",
        "fun",
        "get",
        "if",
        "import",
        "in",
        "infix",
        "init",
        "inline",
        "inner",
        "interface",
        "internal",
        "is",
        "lateinit",
        "noinline",
        "null",
        "object",
        "open",
        "operator",
        "out",
        "override",
        "package",
        "private",
        "protected",
        "public",
        "reified",
        "return",
        "sealed",
        "set",
        "super",
        "suspend",
        "tailrec",
        "this",
        "throw",
        "true",
        "try",
        "typealias",
        "typeof",
        "val",
        "var",
        "vararg",
        "when",
        "where",
        "while",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    triple_quotes: &[],
};

/// TypeScript and JavaScript: one config for both, since JavaScript's
/// keywords are a subset of TypeScript's and a `.js` file that used a
/// TypeScript-only reserved word as an identifier would be unusual enough to
/// not be worth a second table.
const TYPESCRIPT: CLikeConfig = CLikeConfig {
    keywords: &[
        "abstract",
        "any",
        "as",
        "async",
        "await",
        "bigint",
        "boolean",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "declare",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "get",
        "if",
        "implements",
        "import",
        "in",
        "infer",
        "instanceof",
        "interface",
        "is",
        "keyof",
        "let",
        "namespace",
        "never",
        "new",
        "null",
        "number",
        "object",
        "of",
        "package",
        "private",
        "protected",
        "public",
        "readonly",
        "return",
        "set",
        "static",
        "string",
        "super",
        "switch",
        "symbol",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "unknown",
        "var",
        "void",
        "while",
        "with",
        "yield",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    triple_quotes: &[],
};

/// Go.
const GO: CLikeConfig = CLikeConfig {
    keywords: &[
        "bool",
        "break",
        "byte",
        "case",
        "chan",
        "complex128",
        "complex64",
        "const",
        "continue",
        "default",
        "defer",
        "else",
        "error",
        "fallthrough",
        "false",
        "float32",
        "float64",
        "for",
        "func",
        "go",
        "goto",
        "if",
        "import",
        "int",
        "int16",
        "int32",
        "int64",
        "int8",
        "interface",
        "iota",
        "map",
        "nil",
        "package",
        "range",
        "return",
        "rune",
        "select",
        "string",
        "struct",
        "switch",
        "true",
        "type",
        "uint",
        "uint16",
        "uint32",
        "uint64",
        "uint8",
        "uintptr",
        "var",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    triple_quotes: &[],
};

/// Rust. Uppercase library types (`String`, `Vec`, `Option`, `Self`, …) are
/// deliberately not in the table: they are not reserved words, and the
/// PascalCase rule every C-like config shares (§ module docs of
/// [`clike`](crate::lang::clike)) already paints them as [`Token::Type`].
///
/// [`Token::Type`]: crate::highlight::Token::Type
const RUST: CLikeConfig = CLikeConfig {
    keywords: &[
        "as", "async", "await", "bool", "break", "char", "const", "continue", "crate", "dyn",
        "else", "enum", "extern", "f32", "f64", "false", "fn", "for", "i128", "i16", "i32", "i64",
        "i8", "if", "impl", "in", "isize", "let", "loop", "match", "mod", "move", "mut", "pub",
        "ref", "return", "self", "static", "str", "struct", "super", "trait", "true", "type",
        "u128", "u16", "u32", "u64", "u8", "unsafe", "use", "usize", "where", "while",
    ],
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    triple_quotes: &[],
};

/// Python. `False`, `None` and `True` are capitalized exactly as Python
/// reserves them; the keyword table is matched before the PascalCase rule
/// ever runs, so they are keywords and a class `Foo` is still a type.
const PYTHON: CLikeConfig = CLikeConfig {
    keywords: &[
        "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
        "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
        "try", "while", "with", "yield",
    ],
    line_comments: &["#"],
    block_comment: None,
    triple_quotes: &["'''", "\"\"\""],
};

/// The base highlighter for one file extension, if this crate ships one.
///
/// `ext` is matched case-insensitively and without its leading dot — the same
/// form [`std::path::Path::extension`] returns. `None` covers every extension
/// this crate has no lexer for, which an editor draws as plain text.
pub fn highlighter_for_extension(ext: &str) -> Option<Arc<dyn Highlighter>> {
    let ext = ext.to_ascii_lowercase();
    let registry = LanguageRegistry::builtin();
    let entry = registry
        .all()
        .iter()
        .find(|entry| entry.files.matches_extension(&ext))?;
    entry.highlighter.clone()
}

/// A fresh instance of every base [`Highlighter`] this crate ships, in the
/// order [`LanguageRegistry::builtin`] declares them.
///
/// One place rather than a `match` per caller, so that adding a language is one
/// edit. The `Arc`s are built here rather than held in a table because two of
/// them — the shell lexer above all — carry per-document state, and a document
/// wants its own.
fn builtin_highlighter(id: &str) -> Option<Arc<dyn Highlighter>> {
    Some(match id {
        "plain" => return None,
        "shell" => Arc::new(ShellHighlighter::new()),
        "yaml" => Arc::new(YamlHighlighter),
        "json" => Arc::new(JsonHighlighter),
        "toml" => Arc::new(TomlHighlighter),
        "conf" => Arc::new(ConfHighlighter),
        "dockerfile" => Arc::new(DockerfileHighlighter),
        "markdown" => Arc::new(MarkdownHighlighter),
        "sql" => Arc::new(SqlHighlighter),
        "java" => Arc::new(JavaHighlighter),
        "xml" => Arc::new(XmlHighlighter),
        "php" => Arc::new(PhpHighlighter),
        "csharp" => Arc::new(CLikeHighlighter(&CSHARP)),
        "kotlin" => Arc::new(CLikeHighlighter(&KOTLIN)),
        "typescript" => Arc::new(CLikeHighlighter(&TYPESCRIPT)),
        "go" => Arc::new(CLikeHighlighter(&GO)),
        "rust" => Arc::new(CLikeHighlighter(&RUST)),
        "python" => Arc::new(CLikeHighlighter(&PYTHON)),
        _ => return None,
    })
}

/// Test-only helpers shared by every highlighter in this module: running a
/// line through a [`Highlighter`] while checking the span contract it owes
/// its caller (sorted, non-overlapping, inside the line, on char boundaries,
/// never empty), the same checks [`highlight`](crate::highlight)'s own tests
/// hand-roll one at a time.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::highlight::{Highlighter, LineState, Span, Token};

    /// Runs `highlighter` over `line` from `state`, checking the span
    /// contract, and answers with `(text, token)` pairs and the end state.
    pub(crate) fn lex<'a>(
        highlighter: &dyn Highlighter,
        line: &'a str,
        state: LineState,
    ) -> (Vec<(&'a str, Token)>, LineState) {
        let (spans, end) = highlighter.line(line, state);
        check_span_invariants(&spans, line);
        (
            spans
                .iter()
                .map(|span| (&line[span.range.clone()], span.token))
                .collect(),
            end,
        )
    }

    /// Every line of `text`, lexed the way [`crate::highlight::SyntaxCache`]
    /// lexes them: each from the state the line before it ended in.
    pub(crate) fn lex_lines<'a>(
        highlighter: &dyn Highlighter,
        text: &'a str,
    ) -> Vec<Vec<(&'a str, Token)>> {
        let mut state = LineState::START;
        let mut out = Vec::new();
        for line in text.split('\n') {
            let (spans, next) = lex(highlighter, line, state);
            state = next;
            out.push(spans);
        }
        out
    }

    /// Checks that `spans` are sorted, non-overlapping, non-empty, inside
    /// `line`, and cut on character boundaries -- the contract
    /// [`Highlighter::line`] owes its caller.
    pub(crate) fn check_span_invariants(spans: &[Span], line: &str) {
        let mut last = 0;
        for span in spans {
            assert!(
                span.range.start >= last,
                "spans overlap or are unsorted in {line:?}: {spans:?}"
            );
            assert!(
                span.range.end <= line.len(),
                "span past the end of {line:?}: {spans:?}"
            );
            assert!(!span.is_empty(), "empty span in {line:?}: {spans:?}");
            assert!(
                line.is_char_boundary(span.range.start) && line.is_char_boundary(span.range.end),
                "span not on a char boundary in {line:?}: {spans:?}"
            );
            last = span.range.end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every keyword table in this file is sorted -- required for the
    /// binary search each [`CLikeConfig`] is looked up with -- and every
    /// entry is exactly as it is meant to be matched (Python's three
    /// capitalized words aside).
    #[test]
    fn every_keyword_table_is_sorted() {
        for config in [&CSHARP, &KOTLIN, &TYPESCRIPT, &GO, &RUST, &PYTHON] {
            for pair in config.keywords.windows(2) {
                assert!(pair[0] < pair[1], "{pair:?} is out of order");
            }
        }
    }

    #[test]
    fn every_config_fits_the_composable_budget() {
        // Two bits of state today; the assertion is what would catch a future
        // config that needed a third triple-quote slot or more.
        for config in [&CSHARP, &KOTLIN, &TYPESCRIPT, &GO, &RUST, &PYTHON] {
            assert!(config.triple_quotes.len() <= 2);
        }
    }

    #[test]
    fn known_extensions_resolve_case_insensitively() {
        for ext in [
            "java",
            "XML",
            "Html",
            "htm",
            "php",
            "sql",
            "CS",
            "kt",
            "kts",
            "ts",
            "tsx",
            "js",
            "jsx",
            "mjs",
            "cjs",
            "go",
            "rs",
            "py",
            "pyw",
            "json",
            "yaml",
            "yml",
            "md",
            "markdown",
            "properties",
            "ini",
            "cfg",
            "conf",
            "sh",
            "bash",
            "zsh",
            "toml",
            "env",
            "dockerfile",
            "ksh",
            "htm",
        ] {
            assert!(
                highlighter_for_extension(ext).is_some(),
                "{ext} should resolve to a highlighter"
            );
        }
        assert!(highlighter_for_extension("").is_none());
        assert!(highlighter_for_extension("cobol").is_none());
    }

    /// The narrow lookup and the registry are one table, not two that have to
    /// be kept in step by hand.
    #[test]
    fn the_extension_lookup_agrees_with_the_registry() {
        let registry = LanguageRegistry::builtin();
        for entry in registry.all() {
            for extension in &entry.files.extensions {
                let name = format!("file.{extension}");
                assert_eq!(
                    registry.detect(&name, "").id,
                    entry.id,
                    "{extension} is claimed by two languages"
                );
                assert_eq!(
                    highlighter_for_extension(extension).is_some(),
                    entry.highlighter.is_some(),
                    "{extension}"
                );
            }
        }
    }
}
