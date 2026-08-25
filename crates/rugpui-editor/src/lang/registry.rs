//! What a file *is*: the table of languages this crate can colour, and the
//! rules that pick one for a file name.
//!
//! [`highlighter_for_extension`](super::highlighter_for_extension) answers the
//! narrow question — "this file ends in `.yml`, what lexes it" — and is all a
//! host with a path and nothing else needs. A [`LanguageRegistry`] answers the
//! two wider ones: what may a file be *set* to, for the picker every editor
//! grows sooner or later, and what is this file, given that half the shell
//! scripts on a server are called `deploy` rather than `deploy.sh` and a
//! `Dockerfile` has no extension at all.
//!
//! # Detection, in three rules
//!
//! [`LanguageRegistry::detect`] takes a file name and its first line and runs
//! three rules in order, because each is more certain than the one after it:
//! the whole name, the extension, and — only for a name with no extension at
//! all — the `#!` line. The shebang is last because a `.yml` that happens to
//! start with `#!` is still YAML.
//!
//! All three run over the built-in languages first and only then over whatever
//! a host has [`register`](LanguageRegistry::register)ed, so a definition
//! somebody dropped into a directory can *add* a language but never take one
//! over: a `yaml.yml` of their own does not change what a `.yaml` file is.
//!
//! # A value, not a global
//!
//! The registry is an ordinary value. Where it lives — a gpui global, a field
//! on the application's state, one per window — is the host's question, and
//! this crate has no opinion worth encoding about it. Two entries are the same
//! language when their ids match; two highlighters are the same lexer when
//! [`Arc::ptr_eq`] says so, which is what an editor comparing "am I already
//! lexing this" should ask.

use std::sync::Arc;

use gpui::SharedString;

use crate::highlight::Highlighter;
use crate::lang::builtin_highlighter;
use crate::lang::scan::shebang_interpreter;

/// What a language is recognised by.
///
/// Every list is compared in lower case, so a host filling one in may write it
/// however it likes as long as it writes it in lower case — nothing here folds
/// the table, only the name being matched against it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileMatch {
    /// Extensions, with no leading dot: `yml`, `tar.gz` would not work and is
    /// not meant to.
    pub extensions: Vec<String>,
    /// Whole file names: `dockerfile`, `sshd_config`, `.bashrc`.
    ///
    /// A name here also claims everything written as *it* plus a dot and more:
    /// `dockerfile` claims `Dockerfile.build`, and `.env` claims
    /// `.env.production`. That is what the files it is for actually look like,
    /// and it is why these are names rather than extensions.
    pub names: Vec<String>,
    /// What the `#!` interpreter may end with.
    ///
    /// A suffix rather than the whole word, so that one `sh` covers `sh`,
    /// `bash`, `zsh` and `ksh`. A language that wants both `python` and
    /// `python3` has to say both, since neither ends with the other.
    pub shebangs: Vec<String>,
}

impl FileMatch {
    /// Whether `extension`, already lower-cased, is one of these.
    pub fn matches_extension(&self, extension: &str) -> bool {
        self.extensions.iter().any(|known| known == extension)
    }

    /// Whether `name`, already lower-cased and already reduced to its last path
    /// segment, is one of these — or one of these with a dotted tail.
    pub fn matches_name(&self, name: &str) -> bool {
        self.names.iter().any(|known| {
            name == known
                || (name.len() > known.len()
                    && name.starts_with(known.as_str())
                    && name.as_bytes()[known.len()] == b'.')
        })
    }

    /// Whether `interpreter`, already lower-cased and already reduced to its
    /// last path segment, ends with one of these.
    pub fn matches_shebang(&self, interpreter: &str) -> bool {
        self.shebangs
            .iter()
            .any(|known| interpreter.ends_with(known.as_str()))
    }
}

/// One language a registry knows about.
#[derive(Clone)]
pub struct LanguageEntry {
    /// Stable identifier, for settings and for
    /// [`LanguageRegistry::get`]. Lower case by convention.
    pub id: String,
    /// What the language calls itself, for a list a person picks from.
    ///
    /// Proper names, and so the same in every locale: `YAML` and `Dockerfile`
    /// are spelled that way wherever the application is read. The one row worth
    /// translating is plain text, which describes a file rather than naming a
    /// format; a picker that wants to localise it can, since it is drawing the
    /// rows anyway.
    pub name: SharedString,
    /// What files this language claims.
    pub files: FileMatch,
    /// The lexer, or `None` for plain text.
    pub highlighter: Option<Arc<dyn Highlighter>>,
}

impl std::fmt::Debug for LanguageEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageEntry")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("files", &self.files)
            .field("highlighter", &self.highlighter.is_some())
            .finish()
    }
}

/// Every id [`LanguageRegistry::builtin`] declares, in the order it declares
/// them, with the name each one goes by and the files it claims.
///
/// Plain text leads, being the answer to "colour none of this" rather than a
/// format among the others. Then the seven configuration formats a file panel
/// reaches every day, then the languages with a lexer of their own, then the
/// C-like table. That order is the picker's order and the search order at once.
type Builtin = (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

/// The built-in table: `(id, name, extensions, names, shebangs)`.
const BUILTIN: &[Builtin] = &[
    ("plain", "Plain Text", &[], &[], &[]),
    (
        "shell",
        "Shell",
        &["sh", "bash", "zsh", "ksh", "ash", "mksh"],
        &[
            ".bashrc",
            ".bash_profile",
            ".bash_login",
            ".bash_logout",
            ".profile",
            ".zshrc",
            ".zshenv",
            ".zprofile",
            ".zlogin",
            ".zlogout",
            ".kshrc",
            ".shrc",
        ],
        // Anything ending in `sh`: `sh`, `bash`, `zsh`, `ksh`, `dash`, `fish`.
        // `fish` is not bourne shell and is coloured as if it were, which costs
        // a handful of keywords and no correctness — the comments, strings and
        // expansions this highlights are the same in both.
        &["sh"],
    ),
    ("yaml", "YAML", &["yml", "yaml"], &[], &[]),
    ("json", "JSON", &["json"], &[], &[]),
    ("toml", "TOML", &["toml"], &[], &[]),
    (
        "conf",
        "Conf",
        &["ini", "conf", "cfg", "properties", "env"],
        &[
            ".env",
            "sshd_config",
            "ssh_config",
            ".gitconfig",
            ".npmrc",
            ".editorconfig",
        ],
        &[],
    ),
    (
        "dockerfile",
        "Dockerfile",
        &["dockerfile"],
        &["dockerfile", "containerfile"],
        &[],
    ),
    // A bare `README` or `CHANGELOG` is not claimed: it is as often a wall of
    // plain prose as it is Markdown, and plain text is the answer that is never
    // wrong about a file nobody labelled.
    ("markdown", "Markdown", &["md", "markdown"], &[], &[]),
    ("sql", "SQL", &["sql"], &[], &[]),
    ("java", "Java", &["java"], &[], &[]),
    ("xml", "XML", &["xml", "html", "htm"], &[], &[]),
    ("php", "PHP", &["php"], &[], &["php"]),
    ("csharp", "C#", &["cs"], &[], &[]),
    ("kotlin", "Kotlin", &["kt", "kts"], &[], &[]),
    (
        "typescript",
        "TypeScript",
        &["ts", "tsx", "js", "jsx", "mjs", "cjs"],
        &[],
        &["node"],
    ),
    ("go", "Go", &["go"], &[], &[]),
    ("rust", "Rust", &["rs"], &[], &[]),
    (
        "python",
        "Python",
        &["py", "pyw"],
        &[],
        // Both spellings, since neither ends with the other.
        &["python", "python3"],
    ),
];

/// The languages an editor may be set to, and the rules that pick one.
///
/// Built with [`LanguageRegistry::builtin`] and grown with
/// [`LanguageRegistry::register`]. See the module header for where one should
/// live and why this is not a global.
#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    /// Every language, built-in ones first.
    entries: Vec<LanguageEntry>,
    /// How many of `entries` are built in. Everything past this was registered
    /// by the host, and is only consulted once the built-ins have declined.
    builtin: usize,
}

impl LanguageRegistry {
    /// Every language this crate ships, in the order a picker should list them.
    ///
    /// Plain text first, then the configuration formats, then the languages
    /// with a lexer of their own. Built fresh on each call: two registries hold
    /// two sets of highlighters, which is what a lexer carrying per-document
    /// state — [`shell`](crate::lang::shell) — needs.
    pub fn builtin() -> Self {
        let entries: Vec<LanguageEntry> = BUILTIN
            .iter()
            .map(|(id, name, extensions, names, shebangs)| LanguageEntry {
                id: (*id).to_string(),
                name: SharedString::new_static(name),
                files: FileMatch {
                    extensions: extensions.iter().map(|e| (*e).to_string()).collect(),
                    names: names.iter().map(|n| (*n).to_string()).collect(),
                    shebangs: shebangs.iter().map(|s| (*s).to_string()).collect(),
                },
                highlighter: builtin_highlighter(id),
            })
            .collect();
        let builtin = entries.len();
        Self { entries, builtin }
    }

    /// Adds a language of the host's own.
    ///
    /// It goes behind every built-in language, in the order a picker reads —
    /// alphabetically by name among the registered ones. Behind, because a
    /// definition is one file in a directory somebody may have forgotten about
    /// while a built-in is what every other editor colours a `.yml` as; and
    /// alphabetically, because a list of thirty formats is read that way or not
    /// at all.
    ///
    /// An id already registered is not checked for: two entries may share one,
    /// and [`LanguageRegistry::get`] will answer with the first.
    pub fn register(&mut self, entry: LanguageEntry) {
        let key = entry.name.to_lowercase();
        let at = self.entries[self.builtin..]
            .iter()
            .position(|other| other.name.to_lowercase() > key)
            .map_or(self.entries.len(), |offset| self.builtin + offset);
        self.entries.insert(at, entry);
    }

    /// Every language, in the order a picker should list them.
    pub fn all(&self) -> &[LanguageEntry] {
        &self.entries
    }

    /// The language `id` names, if this registry holds one.
    pub fn get(&self, id: &str) -> Option<&LanguageEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The language of a file called `name` whose first line is `first_line`.
    ///
    /// Never fails: a file nothing claims is plain text, which is the answer
    /// that is never wrong. `name` may be a whole path — only its last segment
    /// is read, so a directory called `bin.d` cannot decide what is in it — and
    /// `first_line` may be empty, which simply skips the shebang rule.
    ///
    /// See the module header for the three rules and their order.
    pub fn detect(&self, name: &str, first_line: &str) -> &LanguageEntry {
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let lower = name.to_ascii_lowercase();

        let (builtin, registered) = self.entries.split_at(self.builtin);
        detect_in(builtin, &lower, first_line)
            .or_else(|| detect_in(registered, &lower, first_line))
            .unwrap_or(&self.entries[0])
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

/// The first of `entries` that claims `lower`, by name, then by extension, then
/// by shebang.
fn detect_in<'a>(
    entries: &'a [LanguageEntry],
    lower: &str,
    first_line: &str,
) -> Option<&'a LanguageEntry> {
    if let Some(found) = entries.iter().find(|entry| entry.files.matches_name(lower)) {
        return Some(found);
    }
    // A leading dot marks a hidden file, it is not an extension separator:
    // `.bashrc` splits into an empty stem and `bashrc`, which nobody registers.
    // But it is only the *marker* that says nothing — the rest of the name
    // still can, and `.claude.json` carries as real an extension as
    // `claude.json` does — so the dots come off the front before the split goes
    // looking for one.
    if let Some((_, extension)) = lower.trim_start_matches('.').rsplit_once('.') {
        return entries
            .iter()
            .find(|entry| entry.files.matches_extension(extension));
    }
    let interpreter = shebang_interpreter(first_line)?.to_ascii_lowercase();
    entries
        .iter()
        .find(|entry| entry.files.matches_shebang(&interpreter))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::LineState;
    use crate::lang::test_support::check_span_invariants;

    /// The id `registry` detects `name` as.
    fn detect(registry: &LanguageRegistry, name: &str, first_line: &str) -> String {
        registry.detect(name, first_line).id.clone()
    }

    /// A registry with one extra language in it, named `name` and claiming
    /// whatever `files` says.
    fn with(name: &str, files: FileMatch) -> LanguageRegistry {
        let mut registry = LanguageRegistry::builtin();
        registry.register(LanguageEntry {
            id: name.to_ascii_lowercase(),
            name: name.to_string().into(),
            files,
            highlighter: None,
        });
        registry
    }

    /// A [`FileMatch`] over extensions alone.
    fn extensions(list: &[&str]) -> FileMatch {
        FileMatch {
            extensions: list.iter().map(|e| (*e).to_string()).collect(),
            ..FileMatch::default()
        }
    }

    #[test]
    fn an_extension_names_the_language() {
        let registry = LanguageRegistry::builtin();
        for (name, expected) in [
            ("deploy.sh", "shell"),
            ("run.bash", "shell"),
            ("compose.yml", "yaml"),
            ("k8s.yaml", "yaml"),
            ("package.json", "json"),
            ("Cargo.toml", "toml"),
            ("php.ini", "conf"),
            ("nginx.conf", "conf"),
            ("app.cfg", "conf"),
            ("build.properties", "conf"),
            ("dev.env", "conf"),
            ("app.dockerfile", "dockerfile"),
            ("notes.md", "markdown"),
            ("CHANGELOG.markdown", "markdown"),
            ("schema.sql", "sql"),
            ("Main.java", "java"),
            ("lib.rs", "rust"),
            ("access.log", "plain"),
            // A bare name is not claimed for Markdown: an extensionless
            // `README` is as often plain prose as it is Markdown.
            ("README", "plain"),
        ] {
            assert_eq!(detect(&registry, name, ""), expected, "{name}");
        }
    }

    #[test]
    fn the_extension_is_matched_whatever_its_case() {
        let registry = LanguageRegistry::builtin();
        assert_eq!(detect(&registry, "COMPOSE.YML", ""), "yaml");
        assert_eq!(detect(&registry, "Setup.SH", ""), "shell");
    }

    #[test]
    fn a_name_with_no_extension_is_matched_whole() {
        let registry = LanguageRegistry::builtin();
        for (name, expected) in [
            ("Dockerfile", "dockerfile"),
            ("dockerfile", "dockerfile"),
            ("Dockerfile.build", "dockerfile"),
            ("Containerfile", "dockerfile"),
            ("sshd_config", "conf"),
            (".bashrc", "shell"),
            (".zshrc", "shell"),
            (".profile", "shell"),
            (".env", "conf"),
            (".env.production", "conf"),
            (".gitconfig", "conf"),
        ] {
            assert_eq!(detect(&registry, name, ""), expected, "{name}");
        }
    }

    #[test]
    fn a_dotfile_is_not_all_extension() {
        let registry = LanguageRegistry::builtin();
        // The trap the whole-name table exists for: splitting `.bashrc` on its
        // dot leaves `bashrc`, which is not an extension anybody registers.
        assert_eq!(detect(&registry, ".bash_profile", ""), "shell");
        // And an unknown one falls through to plain rather than to nonsense.
        assert_eq!(detect(&registry, ".unknownrc", ""), "plain");
    }

    #[test]
    fn a_hidden_file_still_gets_its_extension_read() {
        let registry = LanguageRegistry::builtin();
        // Only the leading dot is a hidden-file marker; the rest of the name
        // works the way it does on any other file, so `.claude.json` is as much
        // JSON as `claude.json` is.
        assert_eq!(detect(&registry, ".claude.json", ""), "json");
        assert_eq!(detect(&registry, ".gitlab-ci.yml", ""), "yaml");
        // An unknown extension on a hidden file is still just unknown.
        assert_eq!(detect(&registry, ".config.custom", ""), "plain");
    }

    #[test]
    fn a_shebang_speaks_only_for_a_name_with_no_extension() {
        let registry = LanguageRegistry::builtin();
        for (first_line, expected) in [
            ("#!/bin/sh", "shell"),
            ("#!/bin/bash -e", "shell"),
            ("#!/usr/bin/env zsh", "shell"),
            ("#!/usr/bin/python3", "python"),
            ("#!/usr/bin/env python", "python"),
            ("#!/usr/bin/node", "typescript"),
            // Nothing claims it, so plain text does.
            ("#!/usr/bin/perl", "plain"),
        ] {
            assert_eq!(
                detect(&registry, "deploy", first_line),
                expected,
                "{first_line}"
            );
        }
        // A YAML file that opens with a shebang is still YAML.
        assert_eq!(detect(&registry, "play.yml", "#!/bin/sh"), "yaml");
    }

    #[test]
    fn a_path_is_read_from_its_last_segment() {
        let registry = LanguageRegistry::builtin();
        assert_eq!(detect(&registry, "/etc/nginx/nginx.conf", ""), "conf");
        assert_eq!(detect(&registry, r"C:\src\bin.d\go.sh", ""), "shell");
    }

    #[test]
    fn only_json_and_markdown_refuse_the_comment_toggle() {
        // JSON has nothing a reader would skip; Markdown has nothing either,
        // and its `#` already means "heading".
        let registry = LanguageRegistry::builtin();
        for id in ["shell", "yaml", "toml", "conf", "dockerfile"] {
            let entry = registry.get(id).expect("a built-in language");
            let comment = entry.highlighter.as_ref().expect("a lexer").line_comment();
            assert_eq!(comment, Some("#"), "{id}");
        }
        for id in ["json", "markdown"] {
            let entry = registry.get(id).expect("a built-in language");
            assert_eq!(
                entry.highlighter.as_ref().expect("a lexer").line_comment(),
                None,
                "{id}"
            );
        }
        // Plain text has no lexer at all, and so no toggle.
        assert!(
            registry
                .get("plain")
                .expect("plain text")
                .highlighter
                .is_none()
        );
    }

    #[test]
    fn every_language_keeps_the_span_contract_on_every_line_it_is_given() {
        // One pass over lines drawn from all of the formats in each of them,
        // which is what an editor does the moment somebody opens the wrong
        // file: the guarantee is not that the colours are right but that the
        // spans are sorted, non-overlapping, inside the line and on character
        // boundaries.
        let lines = [
            "",
            "   ",
            "# comment",
            "key: value # trailing",
            "[section]",
            r#"{"a": [1, 2.5, true, null]}"#,
            "RUN echo \"$HOME\" && exit 1",
            "text = \"\"\"open",
            "cat <<'EOF'",
            "a: |",
            "한글 = \"값\" # 주석",
            "🙂🙂🙂",
            "\\\"'`$${}[]<<>>::==",
            "- **half* [a](b <!-- open",
            "```yaml",
            "  ~~~ still inside",
        ];
        for entry in LanguageRegistry::builtin().all() {
            let Some(highlighter) = entry.highlighter.as_ref() else {
                continue;
            };
            // Threaded through, so that each line is also lexed from whatever
            // state the one before it left — which is where a lexer that
            // mishandles its own carry shows up.
            let mut state = LineState::START;
            for line in lines {
                let (spans, next) = highlighter.line(line, state);
                check_span_invariants(&spans, line);
                assert_eq!(
                    next.0 >> LineState::COMPOSABLE_BITS,
                    0,
                    "{} left {next:?} on {line:?}, which is past the composable budget",
                    entry.id
                );
                state = next;
            }
        }
    }

    #[test]
    fn every_built_in_language_names_itself_for_the_picker() {
        // Proper names, spelled the way the formats spell themselves: these
        // rows go into a menu untranslated, so a lowercase `json` or a `Yaml`
        // would be a typo on screen in every locale at once.
        let registry = LanguageRegistry::builtin();
        let names: Vec<&str> = registry
            .all()
            .iter()
            .map(|entry| entry.name.as_ref())
            .collect();
        assert_eq!(
            names,
            [
                "Plain Text",
                "Shell",
                "YAML",
                "JSON",
                "TOML",
                "Conf",
                "Dockerfile",
                "Markdown",
                "SQL",
                "Java",
                "XML",
                "PHP",
                "C#",
                "Kotlin",
                "TypeScript",
                "Go",
                "Rust",
                "Python",
            ]
        );

        // And every id is distinct, since a picker stores one in its settings.
        let mut ids: Vec<&str> = registry.all().iter().map(|e| e.id.as_str()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "two built-in languages share an id");
    }

    #[test]
    fn the_picker_lists_the_built_in_languages_first_and_then_the_rest_by_name() {
        let mut registry = LanguageRegistry::builtin();
        let builtin = registry.all().len();
        for name in ["Zeta", "Alpha"] {
            registry.register(LanguageEntry {
                id: name.to_ascii_lowercase(),
                name: name.to_string().into(),
                files: FileMatch::default(),
                highlighter: None,
            });
        }

        let listed = registry.all();
        assert_eq!(listed.len(), builtin + 2);
        assert_eq!(listed[0].id, "plain", "plain text leads the list");
        assert_eq!(listed[builtin - 1].id, "python", "the built-ins first");
        // Registered in the order the host happened to hand them over, listed
        // in the order a list of formats is read.
        assert_eq!(listed[builtin].name, "Alpha");
        assert_eq!(listed[builtin + 1].name, "Zeta");
    }

    #[test]
    fn a_registered_language_is_detected_only_where_no_builtin_answers() {
        // Claims YAML's extension, which it must not get, and one nothing
        // built in claims, which it must.
        let registry = with("Yaml-ish", extensions(&["yml", "yamlish"]));
        assert_eq!(detect(&registry, "compose.yml", ""), "yaml");
        assert_eq!(detect(&registry, "compose.yamlish", ""), "yaml-ish");
        assert_eq!(detect(&registry, "notes.txt", ""), "plain");
    }

    #[test]
    fn the_first_registered_language_wins_a_shared_extension() {
        let mut registry = with("Aaa", extensions(&["shared"]));
        registry.register(LanguageEntry {
            id: "bbb".into(),
            name: "Bbb".into(),
            files: extensions(&["shared"]),
            highlighter: None,
        });
        // Listed by name, and searched in the order they are listed, so which
        // of two definitions claiming one extension answers is the same on
        // every machine.
        assert_eq!(detect(&registry, "x.shared", ""), "aaa");
    }

    #[test]
    fn a_language_is_looked_up_by_its_id() {
        let registry = LanguageRegistry::builtin();
        assert_eq!(registry.get("yaml").expect("YAML").name, "YAML");
        assert!(registry.get("cobol").is_none());
    }

    #[test]
    fn two_registries_hand_out_two_sets_of_lexers() {
        // A lexer that carries per-document state — the shell one interns
        // heredoc tags — must not be shared between two documents by accident.
        let first = LanguageRegistry::builtin();
        let second = LanguageRegistry::builtin();
        let lexer = |registry: &LanguageRegistry| {
            registry
                .get("shell")
                .expect("shell")
                .highlighter
                .clone()
                .expect("a lexer")
        };
        assert!(!Arc::ptr_eq(&lexer(&first), &lexer(&second)));
        assert!(Arc::ptr_eq(&lexer(&first), &lexer(&first)));
    }
}
