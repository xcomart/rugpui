//! Languages defined by a YAML file rather than by code. *Requires the
//! `custom-syntax` feature.*
//!
//! The lexers beside this one are hand-written because they are worth writing
//! by hand: the formats an editor reaches every day, and the languages whose
//! grammar is genuinely its own. Everything else — the Python script somebody
//! is reading, the `.sql` they are debugging, whatever the machine happens to
//! be running — is served by one general lexer driven by data, because the
//! nineteenth hand-written scanner would be the same shape as the eighteenth
//! and there is no end to the list.
//!
//! # Using it
//!
//! A [`Definition`] is parsed from YAML and turned into a
//! [`LanguageEntry`](crate::lang::LanguageEntry) that a
//! [`LanguageRegistry`](crate::lang::LanguageRegistry) can hold beside the
//! built-in languages:
//!
//! ```no_run
//! use ruui_editor::LanguageRegistry;
//! use ruui_editor::lang::custom::Definition;
//!
//! let mut registry = LanguageRegistry::builtin();
//! for entry in std::fs::read_dir("syntaxes")?.flatten() {
//!     let path = entry.path();
//!     if path.extension().is_none_or(|ext| ext != "yml" && ext != "yaml") {
//!         continue;
//!     }
//!     match Definition::parse(&std::fs::read_to_string(&path)?) {
//!         // The file's stem is a better id than a name slugged from inside
//!         // the file: it is what the user can see and rename.
//!         Ok(mut definition) => {
//!             if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
//!                 definition.id = stem.to_ascii_lowercase();
//!             }
//!             registry.register(definition.into_entry());
//!         }
//!         // One broken definition must not cost the user the others.
//!         Err(err) => log::warn!("skipping {}: {err:#}", path.display()),
//!     }
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! Where the files live, when they are read and whether they are re-read is the
//! host's business, which is why no directory walker ships here. What *is* here
//! is forgiving in the same way the loader above is: a rule inside a file that
//! cannot be honoured — an empty delimiter, a quote that is not one character,
//! a `keywords` group nobody has heard of — is dropped and described rather
//! than failing the file. [`Definition::parse_with_warnings`] hands the
//! complaints back; [`Definition::parse`] logs them.
//!
//! # The schema, whole
//!
//! ```yaml
//! name: Python                 # what the language is called; also its default id
//! files:
//!   extensions: [py, pyi]      # no dot, matched without regard to case
//!   names: [SConstruct]        # exact file names, for what has no extension
//!   shebangs: [python, python3] # matches when the `#!` interpreter ends with this
//! comment: "#"                 # line comment, and what the comment toggle writes
//! block_comment: ["/*", "*/"]  # a comment that may cross lines
//! strings:                     # tried longest opener first
//!   - quote: "'"               # one character; never crosses a line
//!     escape: false            # whether a `\` escapes the next character (default true)
//!   - quote: '"'
//!   - pair: ['"""', '"""']     # an open/close pair, which does cross lines
//! keywords:                    # group name -> how the words in it are coloured
//!   keyword: [def, class, if, else, for, while, return, import]
//!   literal: ["True", "False", "None"]
//! keywords_ignore_case: false  # match keywords whatever their case (default false)
//! variables: ["$"]             # sigils: `$NAME` and `${...}` become variables
//! sections: false              # colour a leading `[section]` as a key
//! keys: none                   # none | colon | equals: colour `key:` / `key=`
//! numbers: true                # colour numeric literals (default true)
//! ```
//!
//! Every key is optional. A file holding nothing but `name` and `files` is
//! legal and gives a language that is matched and drawn in one colour, which is
//! a perfectly good way to start.
//!
//! The four groups `keywords` may name are the tokens a word can reasonably be:
//! `keyword`, `literal`, `key` and `variable`. `literal` is
//! [`Token::Number`](crate::Token) — a palette has one colour for the values a
//! format writes out, and `true` is one of them. A group by any other name is
//! warned about and ignored rather than failing the file.
//! `keywords_ignore_case` covers the whole definition rather than one group,
//! because the languages that need it — SQL above all, where `SELECT` and
//! `select` are the same word — need it everywhere or nowhere.
//!
//! A word YAML would otherwise resolve to something else — `true`, `null`,
//! `NULL` — still arrives as the word it looks like, because the reader hands
//! over the text of a plain scalar wherever a string is wanted. Quoting them
//! anyway is worth doing, since a `literal` list reading
//! `["true", "false", "null"]` says what it means to the next person.
//!
//! # What this cannot express
//!
//! The line is drawn at what a *line-at-a-time* scanner can carry, which is a
//! block comment and a multi-line string and nothing else. Beyond that:
//!
//! * No regular expressions, and no context. A word is a keyword wherever it
//!   stands, and `key:` is a key only at the head of its line, so the keys of
//!   an inline `{a: 1}` are not coloured.
//! * Nesting is not tracked. A block comment ends at the first closing
//!   delimiter, whatever opened in between; the same goes for a `pair` string,
//!   inside which a backslash escapes nothing.
//! * One kind of block comment and one line comment per language.
//! * No heredocs, no indentation-delimited block scalars, no interpolation
//!   coloured inside a string. The languages that need those are the ones with
//!   a lexer of their own.
//!
//! # The state, in seven bits
//!
//! `COMMENT` when a block comment is open, and otherwise the index of the
//! string rule that is — five bits of it, so [`STRING_LIMIT`] rules per
//! definition. Both fit the sixteen
//! [`LineState::COMPOSABLE_BITS`](crate::LineState) allows, so a definition can
//! be the base under an overlay like any built-in language. A definition with
//! more string rules than that keeps the first [`STRING_LIMIT`] and is warned
//! about; a language that spells strings thirty-two ways is a language this
//! module was not built for anyway.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use gpui::SharedString;
use serde::Deserialize;

use crate::highlight::{Highlighter, LineState, Span, Token};
use crate::lang::registry::{FileMatch, LanguageEntry};
use crate::lang::scan::{
    Spans, char_step, number, quote_body, skip_spaces, word_boundary, word_end,
};

/// How many string rules one definition may have.
///
/// The index of the open one is carried in a [`LineState`], five bits of it.
pub const STRING_LIMIT: usize = 32;

/// The state of a line inside a block comment.
const COMMENT: LineState = LineState(1);
/// The tag of a state carrying an open `pair` string; the rule's index is
/// shifted up past it.
const STRING: u32 = 2;
/// How far the string rule's index is shifted up.
const INDEX_SHIFT: u32 = 2;

/// A language read from a definition file, compiled into what the lexer wants.
///
/// Every list is stored in the form the matcher compares against — file names,
/// extensions and shebangs lowercased, keywords sorted — so that detection and
/// lexing do no work per line that could have been done once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// Stable identifier, for settings and for
    /// [`LanguageRegistry::get`](crate::lang::LanguageRegistry::get).
    ///
    /// Public and writable, because the best id is usually the definition
    /// file's stem — what the user can see and rename — and this module never
    /// sees a file name. [`Definition::parse`] fills it in from `name` so that
    /// a caller who has no better answer still gets a usable one.
    pub id: String,
    /// What the definition calls itself, or its id when it calls itself
    /// nothing.
    pub name: String,
    /// Extensions, lowercased, without the dot.
    extensions: Vec<String>,
    /// Whole file names, lowercased.
    names: Vec<String>,
    /// Shebang interpreter suffixes, lowercased.
    shebangs: Vec<String>,
    /// The line comment prefix, which is also what the comment toggle writes.
    comment: Option<String>,
    /// The open and close delimiters of a block comment.
    block: Option<(String, String)>,
    /// String rules, longest opener first so that `"""` is tried before `"`.
    strings: Vec<StringRule>,
    /// Words that are not plain, sorted by the word for a binary search. All
    /// lowercase when `ignore_case`.
    keywords: Vec<(String, Token)>,
    /// Whether a word matches a keyword whatever its case, which is what SQL
    /// needs and what nothing with a compiler wants.
    ignore_case: bool,
    /// The bytes that introduce a variable.
    sigils: Vec<u8>,
    /// Whether a leading `[section]` is a key.
    sections: bool,
    /// Which separator makes the word at the head of a line a key.
    keys: KeyStyle,
    /// Whether numeric literals are coloured.
    numbers: bool,
}

/// One way of writing a string.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StringRule {
    /// A single-character quote, which closes on the line it opened.
    Quote {
        /// The quote character.
        quote: u8,
        /// Whether a `\` escapes the character after it.
        escape: bool,
    },
    /// A delimiter pair, which may cross lines.
    Pair {
        /// What opens it.
        open: String,
        /// What closes it. May be the same as `open`, as `"""` is.
        close: String,
    },
}

impl StringRule {
    /// What has to be matched for this rule to apply.
    fn opener(&self) -> &[u8] {
        match self {
            Self::Quote { quote, .. } => std::slice::from_ref(quote),
            Self::Pair { open, .. } => open.as_bytes(),
        }
    }
}

/// What makes the word at the head of a line a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum KeyStyle {
    /// Nothing does; the language has no mappings.
    #[default]
    None,
    /// `key: value`.
    Colon,
    /// `key = value`.
    Equals,
}

impl KeyStyle {
    /// The style `value` names, or `None` when it names none.
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "colon" => Some(Self::Colon),
            "equals" => Some(Self::Equals),
            _ => None,
        }
    }

    /// The byte that has to follow the word.
    const fn separator(self) -> Option<u8> {
        match self {
            Self::None => None,
            Self::Colon => Some(b':'),
            Self::Equals => Some(b'='),
        }
    }
}

impl Definition {
    /// Reads a definition from `yaml`, logging whatever could not be honoured.
    ///
    /// Fails only when the file is not a definition at all — malformed YAML, or
    /// a key whose value is of the wrong shape. A key this build has never
    /// heard of is ignored, so a file written against a later schema loses that
    /// key rather than the whole language.
    pub fn parse(yaml: &str) -> Result<Self> {
        let (definition, warnings) = Self::parse_with_warnings(yaml)?;
        for warning in warnings {
            log::warn!("the {} syntax definition: {warning}", definition.id);
        }
        Ok(definition)
    }

    /// [`Definition::parse`], with the complaints handed back rather than
    /// logged.
    ///
    /// For a caller that knows which file the definition came from and can say
    /// so, and for a host that ships definitions of its own and wants a test
    /// insisting they need no forgiving.
    pub fn parse_with_warnings(yaml: &str) -> Result<(Self, Vec<String>)> {
        Ok(compile(serde_norway::from_str::<SyntaxFile>(yaml)?))
    }

    /// The registry entry this definition describes.
    pub fn into_entry(self) -> LanguageEntry {
        LanguageEntry {
            id: self.id.clone(),
            name: SharedString::from(self.name.clone()),
            files: FileMatch {
                extensions: self.extensions.clone(),
                names: self.names.clone(),
                shebangs: self.shebangs.clone(),
            },
            highlighter: Some(Arc::new(CustomHighlighter::new(self))),
        }
    }

    /// Whether a line of this language can leave something open for the next
    /// one to finish.
    ///
    /// True of a definition with a block comment or a `pair` string, and of
    /// nothing else: those are the only two things a state carries here.
    pub fn carries_state(&self) -> bool {
        self.block.is_some()
            || self
                .strings
                .iter()
                .any(|rule| matches!(rule, StringRule::Pair { .. }))
    }

    /// The line comment prefix, if the definition names one.
    pub fn line_comment(&self) -> Option<&str> {
        self.comment.as_deref()
    }

    /// How `word` is coloured, when the definition says anything about it.
    ///
    /// Called once per word of every line drawn, so neither branch allocates. A
    /// case-insensitive dictionary is stored lowercased and its search folds
    /// the needle byte by byte as it compares — `str` ordering *is* byte
    /// ordering, so the folded comparison agrees with the order the dictionary
    /// was sorted in. Only ASCII is folded, which is all a keyword can be:
    /// `word_end` stops at anything that is not `[A-Za-z0-9_]`.
    fn keyword(&self, word: &str) -> Option<Token> {
        let found = if self.ignore_case {
            self.keywords.binary_search_by(|(known, _)| {
                known
                    .bytes()
                    .cmp(word.bytes().map(|byte| byte.to_ascii_lowercase()))
            })
        } else {
            self.keywords
                .binary_search_by(|(known, _)| known.as_str().cmp(word))
        };
        found.ok().map(|index| self.keywords[index].1)
    }
}

/// The lexer a [`Definition`] describes.
///
/// Built by [`Definition::into_entry`], or directly by
/// [`CustomHighlighter::new`] for a host that wants the lexer without the
/// registry entry.
#[derive(Debug)]
pub struct CustomHighlighter {
    /// What to lex by.
    definition: Definition,
    /// The line comment, leaked so that it can be handed out as a
    /// `&'static str`.
    ///
    /// [`Highlighter::line_comment`] promises a `'static` string because the
    /// built-in lexers all have one in the binary, and a definition's is a
    /// `String` read at run time. One leak per highlighter that names a
    /// comment, and a highlighter is built when a language is registered rather
    /// than when a document is opened, so the total is the number of
    /// definitions a host loads.
    comment: Option<&'static str>,
}

impl CustomHighlighter {
    /// The lexer `definition` describes.
    pub fn new(definition: Definition) -> Self {
        let comment = definition
            .comment
            .clone()
            .map(|comment| &*Box::leak(comment.into_boxed_str()));
        Self {
            definition,
            comment,
        }
    }

    /// What this lexer was built from.
    pub fn definition(&self) -> &Definition {
        &self.definition
    }
}

impl Highlighter for CustomHighlighter {
    fn line(&self, text: &str, state: LineState) -> (Vec<Span>, LineState) {
        lex(text, state, &self.definition)
    }

    fn line_comment(&self) -> Option<&'static str> {
        self.comment
    }
}

/// The state a `pair` string opened by rule `index` leaves behind.
const fn string_state(index: usize) -> LineState {
    LineState(STRING | ((index as u32) << INDEX_SHIFT))
}

/// The index of the open `pair` string rule, if one is open.
const fn open_string(state: LineState) -> Option<usize> {
    if state.0 & 0b11 == STRING {
        Some((state.0 >> INDEX_SHIFT) as usize)
    } else {
        None
    }
}

/// The spans of one line of `definition`, and the state it leaves behind.
fn lex(line: &str, state: LineState, definition: &Definition) -> (Vec<Span>, LineState) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut spans = Spans::new();
    let mut at = 0;
    // Whether the head-of-line rules — a section header, a key — still apply.
    // They do not to the remainder of a line that began inside something.
    let mut head_rules = true;

    if state == COMMENT {
        head_rules = false;
        let close = definition
            .block
            .as_ref()
            .map(|(_, close)| close.as_bytes())
            .unwrap_or_default();
        match find_end(bytes, 0, close) {
            Some(end) => {
                spans.push(Token::Comment, 0, end);
                at = end;
            }
            None => {
                spans.push(Token::Comment, 0, len);
                return (spans.finish(), state);
            }
        }
    } else if let Some(rule) = open_string(state) {
        head_rules = false;
        let close = match definition.strings.get(rule) {
            Some(StringRule::Pair { close, .. }) => close.as_bytes(),
            _ => &[],
        };
        match find_end(bytes, 0, close) {
            Some(end) => {
                spans.push(Token::String, 0, end);
                at = end;
            }
            None => {
                spans.push(Token::String, 0, len);
                return (spans.finish(), state);
            }
        }
    }

    if head_rules {
        at = head(&mut spans, line, definition);
    }

    while at < len {
        // A block comment beats a line comment when its opener is the longer
        // match, so a language spelling one `#` and the other `#|` is not cut
        // short by the shorter rule. A tie goes to the line comment, which is
        // the simpler reading of an ambiguous pair.
        let line_open = definition
            .comment
            .as_deref()
            .filter(|prefix| starts_at(bytes, at, prefix.as_bytes()));
        let block_open = definition
            .block
            .as_ref()
            .filter(|(open, _)| starts_at(bytes, at, open.as_bytes()));

        if let Some((open, close)) = block_open
            && line_open.is_none_or(|prefix| open.len() > prefix.len())
        {
            match find_end(bytes, at + open.len(), close.as_bytes()) {
                Some(end) => {
                    spans.push(Token::Comment, at, end);
                    at = end;
                }
                None => {
                    spans.push(Token::Comment, at, len);
                    return (spans.finish(), COMMENT);
                }
            }
            continue;
        }
        if line_open.is_some() {
            spans.push(Token::Comment, at, len);
            break;
        }

        if let Some((index, rule)) = string_at(definition, bytes, at) {
            match rule {
                StringRule::Quote { quote, escape } => {
                    // An unterminated one-character quote takes the rest of the
                    // line and nothing more: only a `pair` crosses one.
                    let end = quote_body(line, at + 1, *quote, *escape).unwrap_or(len);
                    spans.push(Token::String, at, end);
                    at = end.max(at + 1);
                }
                StringRule::Pair { open, close } => {
                    match find_end(bytes, at + open.len(), close.as_bytes()) {
                        // Past both delimiters, so `at` moves however long they
                        // are and lands on a character boundary either way.
                        Some(end) => {
                            spans.push(Token::String, at, end);
                            at = end;
                        }
                        None => {
                            spans.push(Token::String, at, len);
                            return (spans.finish(), string_state(index));
                        }
                    }
                }
            }
            continue;
        }

        let byte = bytes[at];
        if definition.sigils.contains(&byte) {
            match variable_end(line, at) {
                Some(end) => {
                    spans.push(Token::Variable, at, end);
                    at = end.max(at + 1);
                }
                None => at += 1,
            }
            continue;
        }
        if definition.numbers && byte.is_ascii_digit() && word_boundary(bytes, at) {
            let end = number(line, at);
            spans.push(Token::Number, at, end);
            at = end.max(at + 1);
            continue;
        }
        if (byte.is_ascii_alphabetic() || byte == b'_') && word_boundary(bytes, at) {
            let end = word_end(bytes, at);
            if let Some(token) = definition.keyword(&line[at..end]) {
                spans.push(token, at, end);
            }
            at = end.max(at + 1);
            continue;
        }
        at += char_step(line, at);
    }

    (spans.finish(), LineState::START)
}

/// Applies the head-of-line rules, and answers where the rest of the line
/// starts.
///
/// A `[section]` runs to the last `]` on the line, as it does in the flat
/// configuration formats — which colours a `]` inside a trailing comment as
/// part of the header, and is the reading that survives the header being typed.
fn head(spans: &mut Spans, line: &str, definition: &Definition) -> usize {
    let bytes = line.as_bytes();
    let head = skip_spaces(bytes, 0);

    if definition.sections && bytes.get(head) == Some(&b'[') {
        let end = line.rfind(']').map_or(bytes.len(), |at| at + 1);
        spans.push(Token::Key, head, end);
        return end.max(head);
    }
    if let Some(separator) = definition.keys.separator() {
        let end = word_end(bytes, head);
        if end > head && bytes.get(skip_spaces(bytes, end)) == Some(&separator) {
            spans.push(Token::Key, head, end);
            return end;
        }
    }
    0
}

/// The end of the `$NAME` or `${...}` whose sigil is at `at`.
///
/// `None` when the sigil introduces nothing, which leaves it as plain text
/// rather than colouring a bare `$` at the end of a line.
fn variable_end(line: &str, at: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    if bytes.get(at + 1) == Some(&b'{') {
        let mut end = at + 2;
        while end < len && bytes[end] != b'}' {
            end += char_step(line, end);
        }
        return Some(if end < len { end + 1 } else { len });
    }
    let end = word_end(bytes, at + 1);
    (end > at + 1).then_some(end)
}

/// The string rule that opens at `at`, and its index.
///
/// The rules are held longest-opener-first, so the first match is the longest
/// one: a definition with both `"""` and `"` opens the triple quote on `"""`
/// rather than an empty string followed by a quote.
fn string_at<'a>(
    definition: &'a Definition,
    bytes: &[u8],
    at: usize,
) -> Option<(usize, &'a StringRule)> {
    definition
        .strings
        .iter()
        .enumerate()
        .find(|(_, rule)| starts_at(bytes, at, rule.opener()))
}

/// Whether `needle` sits at `at` in `haystack`.
///
/// Byte-wise, and safe on any `at`: both ends of a match are character
/// boundaries whenever `at` is one, since `needle` came from a `str`.
fn starts_at(haystack: &[u8], at: usize, needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .get(at..)
            .is_some_and(|rest| rest.starts_with(needle))
}

/// The offset just past the first `needle` at or after `from`.
///
/// An empty needle never matches, which is what keeps a definition that somehow
/// carried one from closing everything immediately. Compiling rejects those, so
/// this is the second lock on the same door.
fn find_end(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    (from..=haystack.len() - needle.len())
        .find(|at| haystack[*at..].starts_with(needle))
        .map(|at| at + needle.len())
}

// --- the file format ---------------------------------------------------------

/// A definition file, as it is written.
///
/// Every field is optional, and unknown fields are ignored — serde's default —
/// so a definition written against a later version of this schema loses the
/// keys this build does not know rather than failing outright.
#[derive(Debug, Clone, Default, Deserialize)]
struct SyntaxFile {
    /// A stable id, when the definition would rather choose one than have it
    /// slugged from its name.
    #[serde(default)]
    id: Option<String>,
    /// What to call the language.
    #[serde(default)]
    name: String,
    /// What the language is recognised by.
    #[serde(default)]
    files: FileMatchers,
    /// The line comment prefix.
    #[serde(default)]
    comment: Option<String>,
    /// The open and close delimiters of a block comment, in that order.
    #[serde(default)]
    block_comment: Option<Vec<String>>,
    /// How strings are written.
    #[serde(default)]
    strings: Vec<StringField>,
    /// Group name to the words in it.
    #[serde(default)]
    keywords: BTreeMap<String, Vec<String>>,
    /// Whether `keywords` matches whatever the case. Absent means no.
    #[serde(default)]
    keywords_ignore_case: Option<bool>,
    /// The sigils that introduce a variable.
    #[serde(default)]
    variables: Vec<String>,
    /// Whether a leading `[section]` is a key.
    #[serde(default)]
    sections: bool,
    /// `none`, `colon` or `equals`. A string rather than an enum so that an
    /// unknown value is a warning about one key instead of a rejected file.
    #[serde(default)]
    keys: Option<String>,
    /// Whether numeric literals are coloured. Absent means yes.
    #[serde(default)]
    numbers: Option<bool>,
}

/// What a definition is recognised by.
#[derive(Debug, Clone, Default, Deserialize)]
struct FileMatchers {
    /// Extensions, with no leading dot.
    #[serde(default)]
    extensions: Vec<String>,
    /// Whole file names.
    #[serde(default)]
    names: Vec<String>,
    /// What the `#!` interpreter may end with.
    #[serde(default)]
    shebangs: Vec<String>,
}

/// One entry of `strings`: either a `quote` or a `pair`, never both.
#[derive(Debug, Clone, Default, Deserialize)]
struct StringField {
    /// A one-character quote.
    #[serde(default)]
    quote: Option<String>,
    /// An open and close delimiter, in that order.
    #[serde(default)]
    pair: Option<Vec<String>>,
    /// Whether a `\` escapes the next character. Absent means yes.
    #[serde(default)]
    escape: Option<bool>,
}

/// The [`Token`] a `keywords` group name asks for.
///
/// Only the tokens a *word* can sensibly be. Colouring a word as a comment or a
/// number is not a thing anybody wants — `literal` is the number colour, but it
/// is asked for by what the word *means* rather than by what it is drawn as —
/// and leaving the rest out keeps the list of legal group names short enough to
/// remember.
fn group_token(group: &str) -> Option<Token> {
    match group.trim().to_ascii_lowercase().as_str() {
        "keyword" => Some(Token::Keyword),
        "literal" => Some(Token::Number),
        "key" => Some(Token::Key),
        "variable" => Some(Token::Variable),
        _ => None,
    }
}

/// An id made out of `name`: lower case, and everything that is not a letter,
/// a digit or a `_` turned into a `-`.
fn slug(name: &str) -> String {
    let slugged: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    slugged.trim_matches('-').to_string()
}

/// Turns a parsed file into the definition the lexer runs on, with everything
/// that had to be dropped along the way.
///
/// Nothing here fails. A rule that cannot be honoured — an empty delimiter, a
/// quote that is not one character, a `keywords` group nobody has heard of — is
/// dropped and described, leaving the rest of the definition working.
fn compile(file: SyntaxFile) -> (Definition, Vec<String>) {
    let mut warnings: Vec<String> = Vec::new();
    let lowercase = |values: Vec<String>| -> Vec<String> {
        values
            .into_iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect()
    };

    let name = file.name.trim().to_string();
    let id = match file.id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => slug(id),
        _ => slug(&name),
    };
    // A file that named neither is a language nothing can refer to; `unnamed`
    // at least keeps it addressable, and the caller is free to overwrite it
    // with a file stem.
    let id = if id.is_empty() {
        "unnamed".to_string()
    } else {
        id
    };
    let name = if name.is_empty() { id.clone() } else { name };

    let block = file.block_comment.and_then(|pair| match pair.as_slice() {
        [open, close] if !open.is_empty() && !close.is_empty() => {
            Some((open.clone(), close.clone()))
        }
        _ => {
            warnings
                .push("block_comment needs an open and a close delimiter, both non-empty".into());
            None
        }
    });

    let mut strings = Vec::new();
    for rule in file.strings {
        match (rule.quote, rule.pair) {
            (Some(quote), None) => {
                let bytes = quote.as_bytes();
                if bytes.len() == 1 && bytes[0].is_ascii() {
                    strings.push(StringRule::Quote {
                        quote: bytes[0],
                        escape: rule.escape.unwrap_or(true),
                    });
                } else {
                    warnings.push(format!(
                        "{quote:?} is not a one-character quote; write it as a pair instead"
                    ));
                }
            }
            (None, Some(pair)) => match pair.as_slice() {
                [open, close] if !open.is_empty() && !close.is_empty() => {
                    strings.push(StringRule::Pair {
                        open: open.clone(),
                        close: close.clone(),
                    });
                }
                _ => warnings.push("a string pair needs an open and a close delimiter".into()),
            },
            _ => warnings
                .push("a string rule is either a quote or a pair, not both or neither".into()),
        }
    }
    if strings.len() > STRING_LIMIT {
        warnings.push(format!(
            "only the first {STRING_LIMIT} string rules are used"
        ));
        strings.truncate(STRING_LIMIT);
    }
    // Longest opener first, so that the `"""` of a definition that also spells
    // `"` is the rule that matches. `sort_by` is stable, so rules of equal
    // length stay in the order they were written.
    strings.sort_by_key(|rule| std::cmp::Reverse(rule.opener().len()));

    // A case-insensitive definition holds its words lowercased, which is what
    // lets the lookup fold the *needle* instead of the dictionary and so stay
    // allocation-free on a path that runs once per word of every drawn line.
    let ignore_case = file.keywords_ignore_case.unwrap_or(false);
    let mut keywords: Vec<(String, Token)> = Vec::new();
    for (group, words) in file.keywords {
        let Some(token) = group_token(&group) else {
            warnings.push(format!("no such keyword group as {group:?}, ignoring it"));
            continue;
        };
        for word in words {
            if word.is_empty() {
                continue;
            }
            let word = if ignore_case {
                word.to_ascii_lowercase()
            } else {
                word
            };
            if keywords.iter().any(|(known, _)| *known == word) {
                warnings.push(format!(
                    "{word:?} is claimed by more than one keyword group"
                ));
                continue;
            }
            keywords.push((word, token));
        }
    }
    keywords.sort_by(|(left, _), (right, _)| left.cmp(right));

    let mut sigils = Vec::new();
    for sigil in file.variables {
        match sigil.as_bytes() {
            [byte] if byte.is_ascii() && !byte.is_ascii_alphanumeric() => sigils.push(*byte),
            _ => warnings.push(format!("{sigil:?} is not a usable variable sigil")),
        }
    }

    let keys = file.keys.map_or(KeyStyle::None, |value| {
        KeyStyle::parse(&value).unwrap_or_else(|| {
            warnings.push(format!("keys is none, colon or equals, not {value:?}"));
            KeyStyle::None
        })
    });

    let definition = Definition {
        id,
        name,
        extensions: lowercase(file.files.extensions)
            .into_iter()
            .map(|extension| extension.trim_start_matches('.').to_string())
            .filter(|extension| !extension.is_empty())
            .collect(),
        names: lowercase(file.files.names),
        shebangs: lowercase(file.files.shebangs),
        comment: file.comment.filter(|comment| !comment.is_empty()),
        block,
        strings,
        keywords,
        ignore_case,
        sigils,
        sections: file.sections,
        keys,
        numbers: file.numbers.unwrap_or(true),
    };
    (definition, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::LanguageRegistry;
    use crate::lang::test_support::check_span_invariants;

    /// A python-shaped definition: the schema exercised end to end.
    const PYTHON: &str = r##"
name: Python
files:
  extensions: [py, PYI]
  names: [SConstruct]
  shebangs: [python, python3]
comment: "#"
strings:
  - quote: "'"
  - quote: '"'
  - pair: ['"""', '"""']
keywords:
  keyword: [def, class, return, import]
  literal: [True, False, None]
"##;

    /// The definition `yaml` describes, with nothing dropped.
    fn definition(yaml: &str) -> Definition {
        let (definition, warnings) = Definition::parse_with_warnings(yaml).expect("a definition");
        assert_eq!(
            warnings,
            Vec::<String>::new(),
            "{} was compiled with complaints",
            definition.id
        );
        definition
    }

    /// The spans of `line` from `state`, as `(text, token)` pairs, with the
    /// state left behind.
    fn lex_line<'a>(
        definition: &Definition,
        line: &'a str,
        state: LineState,
    ) -> (Vec<(&'a str, Token)>, LineState) {
        let (spans, end) = lex(line, state, definition);
        check_span_invariants(&spans, line);
        assert_eq!(
            end.0 >> LineState::COMPOSABLE_BITS,
            0,
            "{line:?} left a state past the composable budget"
        );
        (
            spans
                .iter()
                .map(|span| (&line[span.range.clone()], span.token))
                .collect(),
            end,
        )
    }

    /// The spans of `line` from a clean state.
    fn spans<'a>(definition: &Definition, line: &'a str) -> Vec<(&'a str, Token)> {
        lex_line(definition, line, LineState::START).0
    }

    #[test]
    fn every_state_round_trips_inside_the_composable_budget() {
        for index in [0, 1, 7, STRING_LIMIT - 1] {
            let state = string_state(index);
            assert_eq!(state.0 >> LineState::COMPOSABLE_BITS, 0, "{index}");
            assert_eq!(open_string(state), Some(index));
        }
        assert_eq!(open_string(COMMENT), None);
        assert_eq!(open_string(LineState::START), None);
        assert_ne!(COMMENT, string_state(0));
    }

    #[test]
    fn a_word_that_looks_like_a_yaml_scalar_is_still_a_word() {
        // What the `literal` group of every real definition is made of. If the
        // reader ever resolves these to a boolean or a null instead of handing
        // over their text, every definition loses its literals — so this is the
        // test that would say so.
        let scalars = definition(
            "keywords:\n  literal: [\"true\", \"False\", \"null\", \"NULL\", \"on\", \"~\"]\n",
        );
        for word in ["true", "False", "null", "NULL", "on"] {
            assert_eq!(scalars.keyword(word), Some(Token::Number), "{word}");
        }

        // And unquoted, which is what a person writes first.
        let bare = definition("keywords:\n  literal: [true, False, None, null, NULL]\n");
        for word in ["true", "False", "None", "null", "NULL"] {
            assert_eq!(bare.keyword(word), Some(Token::Number), "{word}");
        }
    }

    #[test]
    fn a_whole_definition_parses() {
        let python = definition(PYTHON);

        assert_eq!(python.id, "python", "the id is slugged from the name");
        assert_eq!(python.name, "Python");
        assert_eq!(python.extensions, ["py", "pyi"]);
        assert_eq!(python.names, ["sconstruct"]);
        assert_eq!(python.shebangs, ["python", "python3"]);
        assert_eq!(python.line_comment(), Some("#"));
        assert!(python.numbers);
        assert_eq!(python.keys, KeyStyle::None);
        assert!(python.carries_state(), "the triple quote crosses a line");
        // The triple quote is first, so it is tried before the single one.
        assert_eq!(python.strings.len(), 3);
        assert_eq!(python.strings[0].opener(), b"\"\"\"");
        assert_eq!(python.keyword("def"), Some(Token::Keyword));
        assert_eq!(python.keyword("None"), Some(Token::Number));
        assert_eq!(python.keyword("none"), None);
    }

    #[test]
    fn a_definition_may_say_almost_nothing() {
        let bare = definition("name: Bare\nfiles:\n  extensions: [bare]\n");
        assert_eq!(bare.name, "Bare");
        assert!(bare.numbers, "numbers default on");
        assert_eq!(bare.line_comment(), None);
        assert!(bare.strings.is_empty());
        assert!(!bare.carries_state());

        // And a file that says nothing at all is still a definition, just one
        // nothing is ever detected as. All it has left is the numbers.
        let empty = definition("{}");
        assert!(empty.extensions.is_empty());
        assert_eq!(empty.id, "unnamed");
        assert_eq!(spans(&empty, "x = 1"), [("1", Token::Number)]);
    }

    #[test]
    fn an_id_may_be_given_rather_than_slugged() {
        assert_eq!(definition("name: My C++\n").id, "my-c");
        assert_eq!(definition("name: My C++\nid: cpp\n").id, "cpp");
    }

    #[test]
    fn a_rule_that_cannot_be_honoured_is_dropped_and_the_rest_kept() {
        let (mixed, warnings) = Definition::parse_with_warnings(
            r#"
name: Mixed
comment: "//"
block_comment: ["/*"]
strings:
  - quote: "''"
  - quote: "'"
  - pair: ["<<", ">>"]
  - {}
keywords:
  keyword: [ok]
  nonsense: [dropped]
variables: ["$", "not a sigil", "a"]
keys: sideways
"#,
        )
        .expect("a definition");

        // The block comment was a one-element list, so there is none.
        assert_eq!(mixed.block, None);
        // Two of the four string rules survive: the two-character quote is not
        // a quote, and the empty rule is neither a quote nor a pair.
        assert_eq!(mixed.strings.len(), 2);
        // The unknown keyword group is gone; the known one is not.
        assert_eq!(mixed.keyword("ok"), Some(Token::Keyword));
        assert_eq!(mixed.keyword("dropped"), None);
        // Only the one-character, non-alphanumeric sigil is usable.
        assert_eq!(mixed.sigils, [b'$']);
        // An unknown key style is no key style.
        assert_eq!(mixed.keys, KeyStyle::None);
        // And what was well-formed still works.
        assert_eq!(mixed.line_comment(), Some("//"));
        // One complaint each for the seven things dropped: the block comment,
        // two string rules, the keyword group, two sigils and the key style.
        assert_eq!(warnings.len(), 7, "{warnings:?}");
    }

    #[test]
    fn a_file_that_is_not_a_definition_is_an_error_rather_than_a_panic() {
        assert!(Definition::parse("name: [not, a, string]").is_err());
        assert!(Definition::parse("\tnot: yaml: at: all").is_err());
        // An unknown key is not an error: a file written for a later schema
        // loses the key rather than the whole definition.
        assert_eq!(
            definition("name: Ahead\nfuture_key: [1, 2]\n").name,
            "Ahead"
        );
    }

    #[test]
    fn a_comment_runs_to_the_end_of_the_line() {
        let python = definition(PYTHON);
        assert_eq!(
            spans(&python, "x = 1  # why"),
            [("1", Token::Number), ("# why", Token::Comment)]
        );
    }

    #[test]
    fn keywords_are_coloured_by_the_group_they_are_in() {
        let python = definition(PYTHON);
        let found = spans(&python, "def run(): return None");
        assert_eq!(found[0], ("def", Token::Keyword));
        assert!(found.contains(&("return", Token::Keyword)));
        assert!(found.contains(&("None", Token::Number)));
        // A word nobody claimed gets no span, and a keyword inside another word
        // is not a keyword.
        assert_eq!(spans(&python, "undefined"), []);
    }

    #[test]
    fn both_quotes_close_on_their_own_line() {
        let python = definition(PYTHON);
        let found = spans(&python, r#"a = 'one' + "two" # done"#);
        assert!(found.contains(&("'one'", Token::String)));
        assert!(found.contains(&(r#""two""#, Token::String)));
        assert!(found.contains(&("# done", Token::Comment)));

        // An unterminated quote takes the rest of the line and carries nothing.
        assert!(
            lex_line(&python, r#"a = "open"#, LineState::START)
                .1
                .is_start()
        );
    }

    #[test]
    fn a_pair_string_carries_until_it_closes() {
        let python = definition(PYTHON);

        let (opened, after) = lex_line(&python, r#"doc = """first"#, LineState::START);
        assert_eq!(opened.last(), Some(&(r#""""first"#, Token::String)));
        assert_eq!(after, string_state(0));

        // The body is a string whatever it holds — a `#` in there is not a
        // comment — and the state does not move.
        let (body, still) = lex_line(&python, "second # not a comment", after);
        assert_eq!(body[0].1, Token::String);
        assert_eq!(still, after);

        let (last, closed) = lex_line(&python, r#"third""" # a comment"#, after);
        assert!(closed.is_start());
        assert_eq!(last[0].1, Token::String);
        assert_eq!(last.last().expect("spans").1, Token::Comment);

        // Opened and closed on one line carries nothing.
        assert!(
            lex_line(&python, r#"d = """one""""#, LineState::START)
                .1
                .is_start()
        );
    }

    #[test]
    fn a_block_comment_carries_until_it_closes() {
        let c = definition(
            r#"
name: C-ish
comment: "//"
block_comment: ["/*", "*/"]
keywords:
  keyword: [int]
"#,
        );

        let (opened, after) = lex_line(&c, "int x; /* open", LineState::START);
        assert_eq!(opened[0].1, Token::Keyword);
        assert_eq!(after, COMMENT);

        let (body, still) = lex_line(&c, "int is not a keyword in here", after);
        assert_eq!(body.len(), 1);
        assert_eq!(body[0].1, Token::Comment);
        assert_eq!(still, after);

        let (last, closed) = lex_line(&c, "*/ int y;", after);
        assert!(closed.is_start());
        assert_eq!(last[0], ("*/", Token::Comment));
        assert!(last.contains(&("int", Token::Keyword)));

        // A block comment that opens and closes on one line leaves the rest of
        // the line alone.
        let found = spans(&c, "int /* aside */ y;");
        assert!(found.contains(&("/* aside */", Token::Comment)));
        assert_eq!(found[0], ("int", Token::Keyword));
    }

    #[test]
    fn only_the_definitions_with_something_to_remember_carry_state() {
        // A `pair` string or a block comment, and nothing else.
        assert!(definition("block_comment: [\"<!--\", \"-->\"]\n").carries_state());
        assert!(definition(PYTHON).carries_state());
        assert!(!definition("comment: \"#\"\nstrings:\n  - quote: \"'\"\n").carries_state());
    }

    #[test]
    fn sections_keys_variables_and_numbers_are_opt_in() {
        let flat = definition(
            r#"
name: Flat
sections: true
keys: equals
variables: ["%"]
numbers: false
"#,
        );

        assert_eq!(spans(&flat, "[group]"), [("[group]", Token::Key)]);
        let found = spans(&flat, "  path = %HOME% 12");
        assert_eq!(found[0], ("path", Token::Key));
        assert!(found.contains(&("%HOME", Token::Variable)));
        // `numbers: false` leaves a number alone.
        assert!(!found.iter().any(|(_, token)| *token == Token::Number));

        // A colon definition does not read an `=` as a mapping, and neither
        // reads a key anywhere but at the head of its line.
        let mapped = definition("name: Mapped\nkeys: colon\n");
        assert!(
            !spans(&mapped, "a = 1")
                .iter()
                .any(|(_, token)| *token == Token::Key)
        );
        let found = spans(&mapped, "key: {inner: 1}");
        assert_eq!(found[0], ("key", Token::Key));
        assert_eq!(
            found.iter().filter(|(_, t)| *t == Token::Key).count(),
            1,
            "only the head of the line is a key"
        );
    }

    #[test]
    fn a_longer_comment_opener_wins_over_a_shorter_one() {
        let both = definition("name: Both\ncomment: \"#\"\nblock_comment: [\"#|\", \"|#\"]\n");
        let found = spans(&both, "#| block |# and # line");
        assert_eq!(found[0], ("#| block |#", Token::Comment));
        assert_eq!(found.last(), Some(&("# line", Token::Comment)));
    }

    #[test]
    fn a_definition_registers_as_a_language_behind_the_built_in_ones() {
        let mut registry = LanguageRegistry::builtin();
        let builtin = registry.all().len();
        registry.register(definition(PYTHON).into_entry());

        assert_eq!(registry.all().len(), builtin + 1);
        let entry = registry.get("python").expect("the registered Python");
        assert_eq!(entry.name, "Python");
        assert_eq!(registry.detect("main.py", "").id, "python");
        assert_eq!(registry.detect("MAIN.PYI", "").id, "python");
        assert_eq!(registry.detect("SConstruct", "").id, "python");
        assert_eq!(registry.detect("/srv/app/tasks.py", "").id, "python");
        assert_eq!(
            registry.detect("build", "#!/usr/bin/env python3").id,
            "python"
        );
        // The built-in table answers first, so a definition claiming a `.yml`
        // would never see one.
        assert_eq!(registry.detect("compose.yml", "").id, "yaml");
        assert_eq!(registry.detect("notes.txt", "").id, "plain");

        // The comment toggle follows the definition, through the trait.
        let lexer = entry.highlighter.as_ref().expect("a lexer");
        assert_eq!(lexer.line_comment(), Some("#"));
        assert!(!lexer.statements());
    }

    #[test]
    fn nothing_here_panics_on_anything() {
        let definitions = [
            definition(PYTHON),
            definition(
                r##"
name: Everything
comment: "#"
block_comment: ["/*", "*/"]
strings:
  - quote: "'"
  - pair: ["<<", ">>"]
variables: ["$"]
sections: true
keys: colon
"##,
            ),
            definition("{}"),
        ];
        let lines = [
            "",
            "   ",
            "#",
            "/*",
            "*/",
            "<<",
            "\"\"\"",
            "'",
            "$",
            "${",
            "[",
            "]",
            ":",
            "키: \"값\" # 주석",
            "🙂🙂🙂",
            "\\\"'`$${}[]<<>>::==",
        ];
        for definition in &definitions {
            // Threaded, so every line is also lexed from whatever the line
            // before it left open — and from states this definition could never
            // have produced, which a swap of the language under a cache would
            // hand it.
            let mut state = LineState::START;
            for line in lines {
                lex_line(definition, line, state);
                lex_line(definition, line, LineState(0xffff));
                state = lex_line(definition, line, state).1;
            }
        }
    }
}
