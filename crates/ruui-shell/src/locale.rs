//! Which interface language to render in, and one check for the files that
//! hold the translations.
//!
//! Deliberately free of any particular localization library. `rust-i18n`
//! compiles a crate's own `locales/*.yml` into *that* crate and keeps the
//! active locale in a process global, so the table an application translates
//! from is the application's and cannot be moved here — which is why
//! [`crate::Strings`] exists at all. What *can* be shared is the arithmetic
//! around it: given the tags an application ships, which one a configured
//! preference and a system locale resolve to.
//!
//! Resolution order, which an application applies at start-up and again
//! whenever its settings dialog saves:
//!
//! 1. the tag stored in the settings file, when the application ships that
//!    language;
//! 2. the operating system's locale, matched loosely (see [`match_tag`]);
//! 3. [`FALLBACK`].
//!
//! Step 3 is normally also the localization library's own compile-time
//! fallback, so a key missing from a translation falls back per key rather than
//! switching the whole interface.
//!
//! # What stays in the application
//!
//! Reading the system locale, applying the answer, and the `t!`-shaped macro
//! everything is translated through. All three are one line each and all three
//! reach into the application's own crate:
//!
//! ```ignore
//! pub fn apply(language: Option<&str>) {
//!     let system = sys_locale::get_locale();
//!     rust_i18n::set_locale(&ruui_shell::locale::resolve(
//!         &tags(),
//!         language,
//!         system.as_deref(),
//!     ));
//! }
//! ```

use std::path::Path;

/// Locale used when neither the settings nor the system offer a supported one.
///
/// Must stay in step with the fallback the application's own localization macro
/// was given.
pub const FALLBACK: &str = "en";

/// The locale to render the interface in.
///
/// `available` is the set of tags the application ships, in the order it wants
/// them preferred — sorted, conventionally, which is what makes the
/// primary-subtag rule below deterministic. `preferred` is what the settings
/// file says and `system` what the platform reports; a blank string, a `None`
/// and a tag nothing matches are all the same answer, and fall through.
///
/// Answers [`FALLBACK`] when nothing matches, whether or not `available`
/// contains it: a caller with no translations at all still gets a locale to
/// hand to its library, which will then fall back per key.
pub fn resolve(available: &[&str], preferred: Option<&str>, system: Option<&str>) -> String {
    if let Some(tag) = preferred.and_then(|tag| match_tag(available, tag)) {
        return tag.to_owned();
    }
    system
        .and_then(|tag| match_tag(available, tag))
        .unwrap_or(FALLBACK)
        .to_owned()
}

/// Matches one locale identifier against the tags an application ships.
///
/// Deliberately forgiving, because the string can come from a hand-edited
/// settings file or from a platform that spells locales its own way: case is
/// ignored, the POSIX `_` separator is accepted alongside `-`, and any trailing
/// encoding or modifier suffix (`ko_KR.UTF-8`, `de_DE@euro`) is cut off.
///
/// A tag with no exact match falls back to the first shipped locale — first in
/// `available` order — sharing its primary subtag, so `ko-KR` finds `ko`,
/// `en-GB` finds `en`, and `zh-TW` finds `zh-CN` for as long as Simplified
/// Chinese is the only Chinese translation shipped. Adding `zh-TW` would take
/// over that exact tag, while the remaining `zh-*` regions would keep landing
/// on `zh-CN`.
pub fn match_tag<'a>(available: &[&'a str], tag: &str) -> Option<&'a str> {
    let normalized: String = tag
        .trim()
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if let Some(exact) = available
        .iter()
        .find(|code| code.eq_ignore_ascii_case(&normalized))
    {
        return Some(exact);
    }

    let primary = normalized.split('-').next().unwrap_or_default();
    available
        .iter()
        .find(|code| {
            code.split('-')
                .next()
                .is_some_and(|shipped| shipped.eq_ignore_ascii_case(primary))
        })
        .copied()
}

/// The endonym of `tag`, given the `(tag, endonym)` pairs an application built
/// from its own locale files.
///
/// A helper for the settings dialog's language picker, and the reason the pairs
/// are built rather than hardcoded: the endonym comes from each file's own
/// `language.name`, is written in the language it names, and is deliberately
/// not translated.
pub fn display_name<'a>(supported: &'a [(&'a str, String)], tag: &str) -> Option<&'a str> {
    supported
        .iter()
        .find(|(code, _)| *code == tag)
        .map(|(_, name)| name.as_str())
}

/// Every `key: value` pair of one locale file, as a dotted path.
///
/// A hand-rolled reader rather than a YAML dependency: these files are two or
/// three levels of plain `key: value` by construction, and the two properties
/// the checks below need — the key set, and whether a value is a scalar YAML
/// would swallow — do not need a parser. It reads the *files*, not the
/// compiled-in table, which is the point: a per-key fallback makes a missing
/// key look like a working lookup, and a value YAML swallowed looks like an
/// empty string with nothing to say where it went.
///
/// # Panics
///
/// Panics when the file cannot be read. Only tests call this, and a locale file
/// that is not there is exactly what such a test exists to notice.
pub fn pairs(dir: &Path, tag: &str) -> Vec<(String, String)> {
    let path = dir.join(format!("{tag}.yml"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    parse_pairs(&text)
}

/// The pairs of one locale file's text; see [`pairs`].
fn parse_pairs(text: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let depth = (line.len() - trimmed.len()) / 2;
        stack.truncate(depth);
        stack.push(key.to_string());
        let value = value.trim();
        if !value.is_empty() {
            pairs.push((stack.join("."), value.to_string()));
        }
    }
    pairs
}

/// Scalars YAML reads as something other than the text they look like.
///
/// The one that got through in practice: `nullable: Null` is YAML's *null
/// literal*, so the column heading loaded as an empty string and the tab drew a
/// blank. Every other keyword scalar has the same trap.
const YAML_KEYWORDS: [&str; 22] = [
    "null", "Null", "NULL", "~", "true", "True", "TRUE", "false", "False", "FALSE", "yes", "Yes",
    "YES", "no", "No", "NO", "on", "On", "ON", "off", "Off", "OFF",
];

/// Checks a directory of locale files against the two mistakes that never show
/// up in a running application.
///
/// Meant to be called from one test in the application that owns the files:
///
/// ```ignore
/// #[test]
/// fn the_locale_files_are_sound() {
///     let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
///     ruui_shell::locale::check_locale_dir(&dir, &tags());
/// }
/// ```
///
/// Two properties, both invisible on screen:
///
/// * **Every locale carries exactly the keys [`FALLBACK`] does.** A key missing
///   from a translation is answered in English by the per-key fallback, so it
///   looks like a working lookup; a key *extra* in one is a rename nobody
///   finished.
/// * **No value is a bare YAML keyword or a bare number.** Both load as
///   something other than their text — the first as a null or a boolean, the
///   second as a number rendered without its formatting — and both look like a
///   line that was simply never written.
///
/// `_version`, which localization tooling writes as a bare number on purpose,
/// is exempt from the second.
///
/// # Panics
///
/// Panics, naming the file and the key, on the first thing it finds wrong.
/// That is the point: it is an assertion, not a report.
pub fn check_locale_dir(dir: &Path, tags: &[&str]) {
    assert!(
        tags.contains(&FALLBACK),
        "the fallback locale {FALLBACK:?} ships no file of its own"
    );

    let english: Vec<String> = pairs(dir, FALLBACK)
        .into_iter()
        .map(|(key, _)| key)
        .collect();

    for tag in tags {
        let theirs = pairs(dir, tag);

        for (key, value) in &theirs {
            // `_version` is a number on purpose; everything else is text.
            if key == "_version" {
                continue;
            }
            assert!(
                !YAML_KEYWORDS.contains(&value.as_str()),
                "{tag}: {key} is the bare YAML keyword {value:?} and loads as nothing; quote it"
            );
            assert!(
                value.parse::<f64>().is_err(),
                "{tag}: {key} is the bare number {value:?} and loads as one; quote it"
            );
        }

        if *tag == FALLBACK {
            continue;
        }
        let keys: Vec<&String> = theirs.iter().map(|(key, _)| key).collect();
        let missing: Vec<&String> = english.iter().filter(|key| !keys.contains(key)).collect();
        let extra: Vec<&&String> = keys.iter().filter(|key| !english.contains(key)).collect();
        assert!(missing.is_empty(), "{tag} is missing {missing:?}");
        assert!(
            extra.is_empty(),
            "{tag} has {extra:?}, which {FALLBACK}.yml does not"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shipped set in the tag order an application would build it in.
    const SHIPPED: [&str; 6] = ["de", "en", "es", "fr", "ja", "ko"];

    /// The same, with one region-qualified tag and no plain `zh`.
    const WITH_REGION: [&str; 3] = ["en", "ko", "zh-CN"];

    #[test]
    fn every_shipped_tag_matches_itself() {
        for tag in SHIPPED {
            assert_eq!(match_tag(&SHIPPED, tag), Some(tag));
        }
        for tag in WITH_REGION {
            assert_eq!(match_tag(&WITH_REGION, tag), Some(tag));
        }
    }

    #[test]
    fn matching_ignores_case_and_the_posix_separator() {
        assert_eq!(match_tag(&SHIPPED, "KO"), Some("ko"));
        assert_eq!(match_tag(&SHIPPED, "  ja  "), Some("ja"));
        assert_eq!(match_tag(&WITH_REGION, "zh_cn"), Some("zh-CN"));
        assert_eq!(match_tag(&WITH_REGION, "ZH-Hans-CN"), Some("zh-CN"));
    }

    #[test]
    fn matching_falls_back_to_the_primary_subtag() {
        assert_eq!(match_tag(&SHIPPED, "ko-KR"), Some("ko"));
        assert_eq!(match_tag(&SHIPPED, "en-GB"), Some("en"));
        assert_eq!(match_tag(&SHIPPED, "es-419"), Some("es"));
        assert_eq!(match_tag(&SHIPPED, "fr_CA.UTF-8"), Some("fr"));
        assert_eq!(match_tag(&SHIPPED, "de_DE@euro"), Some("de"));
    }

    #[test]
    fn a_region_with_no_file_of_its_own_takes_the_first_of_its_language() {
        assert_eq!(match_tag(&WITH_REGION, "zh"), Some("zh-CN"));
        assert_eq!(match_tag(&WITH_REGION, "zh-TW"), Some("zh-CN"));
        assert_eq!(match_tag(&WITH_REGION, "zh-Hant-HK"), Some("zh-CN"));
    }

    #[test]
    fn an_unknown_or_empty_tag_matches_nothing() {
        assert_eq!(match_tag(&SHIPPED, ""), None);
        assert_eq!(match_tag(&SHIPPED, "   "), None);
        assert_eq!(match_tag(&SHIPPED, "xx-YZ"), None);
        assert_eq!(match_tag(&SHIPPED, "kor"), None);
        // A prefix of a shipped tag is still a different language.
        assert_eq!(match_tag(&SHIPPED, "e"), None);
        assert_eq!(match_tag(&[], "en"), None);
    }

    #[test]
    fn a_configured_language_wins_over_the_system_locale() {
        assert_eq!(resolve(&SHIPPED, Some("ko"), Some("ja")), "ko");
        assert_eq!(resolve(&WITH_REGION, Some("zh_TW"), Some("en")), "zh-CN");
    }

    #[test]
    fn the_system_locale_is_consulted_only_when_the_setting_says_nothing() {
        for preferred in [None, Some(""), Some("xx-YZ")] {
            assert_eq!(resolve(&SHIPPED, preferred, Some("ja_JP.UTF-8")), "ja");
        }
    }

    #[test]
    fn nothing_matching_anywhere_is_the_fallback() {
        assert_eq!(resolve(&SHIPPED, None, None), FALLBACK);
        assert_eq!(resolve(&SHIPPED, Some("xx"), Some("yy")), FALLBACK);
        // Even with no translations at all, a caller gets a locale to pass on.
        assert_eq!(resolve(&[], Some("ko"), Some("ko")), FALLBACK);
    }

    #[test]
    fn an_endonym_is_found_by_its_tag_and_never_invented() {
        let supported = vec![("en", "English".to_string()), ("ko", "한국어".to_string())];
        assert_eq!(display_name(&supported, "ko"), Some("한국어"));
        assert_eq!(display_name(&supported, "ja"), None);
    }

    #[test]
    fn the_reader_flattens_the_nesting_and_drops_the_headings() {
        let text = "\
_version: 2
common:
  close: Close
  cancel: Cancel
about:
  title: About
";
        assert_eq!(
            parse_pairs(text),
            vec![
                ("_version".to_string(), "2".to_string()),
                ("common.close".to_string(), "Close".to_string()),
                ("common.cancel".to_string(), "Cancel".to_string()),
                ("about.title".to_string(), "About".to_string()),
            ]
        );
    }

    #[test]
    fn comments_blank_lines_and_list_items_are_not_pairs() {
        let text = "\
# a comment
common:

  close: Close
  - not a key: value
";
        assert_eq!(
            parse_pairs(text),
            vec![("common.close".to_string(), "Close".to_string())]
        );
    }

    #[test]
    fn a_directory_of_sound_files_passes_and_every_mistake_is_caught() {
        let dir = tempfile::tempdir().expect("a temp directory");
        let write = |tag: &str, body: &str| {
            std::fs::write(dir.path().join(format!("{tag}.yml")), body).expect("a locale file");
        };

        write(
            "en",
            "_version: 2\ngrid:\n  nullable: \"Null\"\n  close: Close\n",
        );
        write("ko", "_version: 2\ngrid:\n  nullable: 널\n  close: 닫기\n");
        check_locale_dir(dir.path(), &["en", "ko"]);

        // A key the translation never got.
        write("ko", "_version: 2\ngrid:\n  close: 닫기\n");
        assert!(
            std::panic::catch_unwind(|| check_locale_dir(dir.path(), &["en", "ko"])).is_err(),
            "a missing key has to be caught"
        );

        // A key nothing else has.
        write(
            "ko",
            "_version: 2\ngrid:\n  nullable: 널\n  close: 닫기\n  stale: 옛것\n",
        );
        assert!(
            std::panic::catch_unwind(|| check_locale_dir(dir.path(), &["en", "ko"])).is_err(),
            "a leftover key has to be caught"
        );

        // The unquoted keyword this check exists for.
        write(
            "ko",
            "_version: 2\ngrid:\n  nullable: Null\n  close: 닫기\n",
        );
        assert!(
            std::panic::catch_unwind(|| check_locale_dir(dir.path(), &["en", "ko"])).is_err(),
            "a bare YAML keyword has to be caught"
        );

        // And a bare number, which loads without its formatting.
        write("ko", "_version: 2\ngrid:\n  nullable: 1.0\n  close: 닫기\n");
        assert!(
            std::panic::catch_unwind(|| check_locale_dir(dir.path(), &["en", "ko"])).is_err(),
            "a bare number has to be caught"
        );
    }

    #[test]
    fn a_set_without_the_fallback_is_refused_outright() {
        let dir = tempfile::tempdir().expect("a temp directory");
        std::fs::write(dir.path().join("ko.yml"), "a: b\n").expect("a locale file");
        assert!(
            std::panic::catch_unwind(|| check_locale_dir(dir.path(), &["ko"])).is_err(),
            "the fallback has to ship a file"
        );
    }
}
