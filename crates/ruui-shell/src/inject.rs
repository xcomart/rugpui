//! What the host application has to hand the shell before any of it works.
//!
//! Everything in this crate is written against an application it deliberately
//! knows nothing about: its name, the repository it publishes releases from, the
//! words it says to the user, and where it keeps the "never mention this
//! version again" tag are all injected. Three things carry that:
//!
//! * [`AppIdentity`] — the constants. Installed once with [`init`].
//! * [`Strings`] — the user-visible words, looked up by the very keys the
//!   application's own locale files already carry. Installed with
//!   [`set_strings`].
//! * [`UpdatePolicy`] — reading and writing the ignored-release tag, which
//!   lives in the application's settings file. Installed with
//!   [`set_update_policy`].
//!
//! # Why the identity is not only a gpui global
//!
//! The update check and the install run on the background executor, where there
//! is no [`App`] to read a global out of. [`init`] therefore records the
//! identity twice: in a gpui global, which is what [`identity`] answers from,
//! and in a process-wide slot the background paths read through
//! [`current_identity`]. Both are set by the same call, so they cannot drift.

use std::sync::RwLock;

use gpui::{App, Global, SharedString};

/// Everything about the host application the shell has to be told.
///
/// Every field is a constant of the application, which is why they are all
/// `&'static str`: the shell never composes one, it only reads them. The
/// platform-dependent ones — [`AppIdentity::payload`] above all — are chosen by
/// the application under its own `cfg`, because only the application knows what
/// its release archives carry.
#[derive(Debug, Clone, Copy)]
pub struct AppIdentity {
    /// The application's own name, lower-case and without spaces, as it appears
    /// in a release asset's file name and in the `User-Agent` the shell sends.
    ///
    /// Also the wordmark the about dialog draws.
    pub name: &'static str,
    /// The running build's version, which is the application's
    /// `env!("CARGO_PKG_VERSION")` and can only be read there.
    ///
    /// The shell has a version of its own and it is not this one, so this
    /// always has to be passed in: a shell that reported its own version would
    /// tell the user the wrong thing and compare the wrong number against the
    /// latest release.
    pub version: &'static str,
    /// Project home page, opened by the about dialog's repository button.
    pub repository_url: &'static str,
    /// Label of that button; conventionally the URL without its scheme.
    pub repository_label: &'static str,
    /// The GitHub API endpoint answering with the most recent non-draft,
    /// non-prerelease release of the project.
    pub latest_release_api: &'static str,
    /// Where "Update" goes when the API answered without an `html_url`.
    pub releases_page: &'static str,
    /// Fallback name for the downloaded archive, used when the asset name the
    /// API reported is not a plain file name.
    pub fallback_archive: &'static str,
    /// What a release archive holds that has to end up on disk, in install
    /// order.
    ///
    /// The first entry is always the executable — or, on macOS, the application
    /// bundle — because that is the one whose *installed* name may differ from
    /// the published one. A single-file application passes a one-element slice;
    /// nothing else about the install changes.
    pub payload: &'static [&'static str],
    /// Where the executable sits inside the macOS bundle, e.g.
    /// `Contents/MacOS/<name>`.
    ///
    /// Read only when a staged update is applied on macOS, which needs
    /// something runnable out of a plan that names the bundle.
    pub bundle_executable: &'static str,
    /// The "Apps & features" entry a Windows installer leaves behind, relative
    /// to `HKEY_CURRENT_USER` or `HKEY_LOCAL_MACHINE`.
    ///
    /// Inno Setup derives the key name from its `AppId` by appending `_is1`, so
    /// this is that GUID in that shape. It is a published identifier of the
    /// application and is emphatically not the shell's to invent: two
    /// applications sharing one would have winget treat an installed copy of
    /// either as an installed copy of the other.
    pub windows_arp_key: &'static str,
    /// Whether the renames an install performs have to be left to the next
    /// launch.
    ///
    /// A question only the application can answer, because the answer is about
    /// what it has loaded: on Windows a JVM in the process holds open handles
    /// on the very files the swap renames. An application that loads nothing of
    /// the kind passes a function answering `false`.
    pub must_defer: fn() -> bool,
}

/// The identity as a gpui global.
struct Identity(AppIdentity);

impl Global for Identity {}

/// The identity as the background paths can reach it; see the module docs.
static PROCESS_IDENTITY: RwLock<Option<AppIdentity>> = RwLock::new(None);

/// Installs `identity` for the rest of the process.
///
/// Call once, before the first window opens and before anything starts an
/// update check.
pub fn init(identity: AppIdentity, cx: &mut App) {
    init_process_identity(identity);
    cx.set_global(Identity(identity));
}

/// The identity the host installed.
///
/// # Panics
///
/// Panics when [`init`] has not run. Every caller is inside a window the
/// application opened after installing it, so reaching this is a wiring
/// mistake rather than a runtime condition.
pub fn identity(cx: &App) -> AppIdentity {
    cx.global::<Identity>().0
}

/// The identity, off the UI thread.
///
/// # Panics
///
/// Panics when [`init`] has not run, for the same reason [`identity`] does.
pub(crate) fn current_identity() -> AppIdentity {
    PROCESS_IDENTITY
        .read()
        .expect("the identity lock is never held across a panic")
        .expect("ruui_shell::init has to run before the update paths do")
}

/// Installs `identity` for the background paths alone, without an [`App`].
///
/// [`crate::update::apply_pending`] and [`crate::update::clean_leftovers`]
/// read [`current_identity`] and touch no [`App`] — deliberately, since a host
/// that has to apply a staged update or sweep up a previous one's leftovers
/// has to do that *before* it can build one. On a platform where building the
/// application loads something the update paths must not race — a JVM behind
/// `gpui_platform::application()`, say — call this first, ahead of
/// `application()` and `app.run` both, so those two functions have an identity
/// to read. [`init`] calls this itself and additionally installs the gpui
/// global [`identity`] reads, so a host that reaches an [`App`] before it
/// needs the update paths never has to call this directly.
///
/// Safe to call more than once, including once here and again from [`init`]:
/// each call simply replaces whatever the last one installed, and installing
/// the same identity twice leaves nothing different behind.
pub fn init_process_identity(identity: AppIdentity) {
    *PROCESS_IDENTITY
        .write()
        .expect("the identity lock is never held across a panic") = Some(identity);
}

/// The words the shell shows the user.
///
/// Keys are the host application's own — `common.close`, `update.available`,
/// `settings.manage.import` and the rest — so an application adopting the shell
/// changes no locale file. The implementation is one line over whatever the
/// application already translates with; nothing here depends on `rust-i18n` or
/// on any other particular library.
///
/// Interpolation is *not* the implementation's job: a template comes back with
/// its `%{placeholder}` markers intact and [`text`] fills them in. That is what
/// lets the shell interpolate values the application's key never mentioned.
pub trait Strings: 'static {
    /// The translation of `key` in the language currently in force.
    fn text(&self, key: &str) -> SharedString;
}

/// The installed [`Strings`], as a gpui global.
struct Words(Box<dyn Strings>);

impl Global for Words {}

/// Installs the table the shell looks its words up in.
///
/// Call once at start-up. Calling it again — after a language change, say — is
/// harmless but usually unnecessary: an implementation that reads the active
/// locale at each lookup follows a language change on its own.
pub fn set_strings(strings: Box<dyn Strings>, cx: &mut App) {
    cx.set_global(Words(strings));
}

/// The translation of `key`, with `%{name}` markers replaced from `args`.
///
/// A key the host has no translation for comes back as the key itself, which is
/// both visible on screen and harmless — the alternative, a panic inside a
/// render pass, would take the window down over a missing line of text.
///
/// A marker with no matching argument is left as it stands, for the same
/// reason.
pub fn text(cx: &App, key: &str, args: &[(&str, &str)]) -> SharedString {
    let Some(words) = cx.try_global::<Words>() else {
        return SharedString::from(key.to_owned());
    };
    let template = words.0.text(key);
    if args.is_empty() {
        return template;
    }
    SharedString::from(interpolate(&template, args))
}

/// The translation of `key`, with nothing to fill in.
pub fn label(cx: &App, key: &str) -> SharedString {
    text(cx, key, &[])
}

/// Replaces every `%{name}` in `template` that `args` names.
///
/// Written as one pass over the template rather than as a `replace` per
/// argument, so that a value which itself contains a marker cannot be
/// substituted into a second time.
fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("%{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            // An unterminated marker is text, not a placeholder.
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        match args.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[start..start + 2 + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Where the "never tell me about this release again" tag is kept.
///
/// The tag belongs in the application's settings file, which the shell does not
/// own and cannot write; this is the two-line window onto it. Both halves run
/// on the UI thread, so an implementation may read and write the application's
/// settings global directly.
pub trait UpdatePolicy: 'static {
    /// The release tag the user asked never to be told about again.
    fn ignored(&self, cx: &App) -> Option<String>;
    /// Records `tag` as that release, or clears the record with `None`.
    ///
    /// Expected to persist immediately rather than at the next save: this is a
    /// decision the user has just made in a dialog, and it should survive a
    /// crash the way a saved setting does.
    fn set_ignored(&self, tag: Option<String>, cx: &mut App);
}

/// The installed [`UpdatePolicy`], as a gpui global.
struct Policy(Box<dyn UpdatePolicy>);

impl Global for Policy {}

/// Installs the window onto the ignored-release tag.
pub fn set_update_policy(policy: Box<dyn UpdatePolicy>, cx: &mut App) {
    cx.set_global(Policy(policy));
}

/// The release tag the user has asked never to be told about again.
///
/// `None` when there is none, and also when the host installed no policy at
/// all — an application that does not offer "ignore this version" simply never
/// suppresses anything.
pub fn ignored_release(cx: &App) -> Option<String> {
    cx.try_global::<Policy>()
        .and_then(|policy| policy.0.ignored(cx))
}

/// Records `tag` as the release never to mention again.
pub fn set_ignored_release(tag: Option<String>, cx: &mut App) {
    if !cx.has_global::<Policy>() {
        log::debug!("no update policy is installed; the ignored release was not recorded");
        return;
    }
    // Taken out of the global for the call, because the implementation is
    // expected to write the application's settings — which is a `&mut App` of
    // its own, and cannot be taken while the global is borrowed.
    let policy = std::mem::replace(
        &mut cx.global_mut::<Policy>().0,
        Box::new(NoPolicy) as Box<dyn UpdatePolicy>,
    );
    policy.set_ignored(tag, cx);
    cx.global_mut::<Policy>().0 = policy;
}

/// Stand-in installed for the moment [`set_ignored_release`] has the real one
/// in hand. Never reached: nothing runs between the two swaps.
struct NoPolicy;

impl UpdatePolicy for NoPolicy {
    fn ignored(&self, _cx: &App) -> Option<String> {
        None
    }

    fn set_ignored(&self, _tag: Option<String>, _cx: &mut App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_is_replaced_by_the_argument_that_names_it() {
        assert_eq!(
            interpolate(
                "%{app} %{version} is available.",
                &[("app", "widget"), ("version", "1.2")]
            ),
            "widget 1.2 is available."
        );
    }

    #[test]
    fn a_template_with_nothing_to_fill_in_comes_through_unchanged() {
        assert_eq!(interpolate("Close", &[("version", "1.2")]), "Close");
    }

    #[test]
    fn a_marker_nobody_named_is_left_as_it_stands() {
        // Visible, and therefore reportable, which a silently blank line is
        // not.
        assert_eq!(
            interpolate("%{a} and %{b}", &[("a", "one")]),
            "one and %{b}"
        );
        assert_eq!(
            interpolate("%{unterminated", &[("a", "one")]),
            "%{unterminated"
        );
    }

    #[test]
    fn a_value_that_looks_like_a_marker_is_not_substituted_into() {
        assert_eq!(
            interpolate("%{outer}", &[("outer", "%{inner}"), ("inner", "no")]),
            "%{inner}"
        );
    }

    #[test]
    fn whitespace_inside_a_marker_is_ignored() {
        assert_eq!(interpolate("%{ name }", &[("name", "value")]), "value");
    }
}

#[cfg(test)]
mod app_tests {
    use gpui::TestAppContext;

    use super::*;

    /// A table with one line in it, and a marker in that line.
    struct Words;

    impl Strings for Words {
        fn text(&self, key: &str) -> SharedString {
            match key {
                "common.close" => "Close".into(),
                "update.available" => "%{app} %{version} is available.".into(),
                _ => SharedString::from(key.to_owned()),
            }
        }
    }

    /// The tag as a settings file would hold it.
    struct Policy(std::sync::Arc<std::sync::Mutex<Option<String>>>);

    impl UpdatePolicy for Policy {
        fn ignored(&self, _cx: &App) -> Option<String> {
            self.0.lock().expect("no panic while held").clone()
        }

        fn set_ignored(&self, tag: Option<String>, _cx: &mut App) {
            *self.0.lock().expect("no panic while held") = tag;
        }
    }

    /// A plausible application, so that no test here names a real one.
    ///
    /// Deliberately identical to the one `crate::update`'s tests install: the
    /// process holds one identity and gpui's test runner threads, so two that
    /// differed would race.
    const FAKE: AppIdentity = AppIdentity {
        name: "widget",
        version: "0.2.0",
        repository_url: "https://example.invalid/widget",
        repository_label: "example.invalid/widget",
        latest_release_api: "https://example.invalid/api/releases/latest",
        releases_page: "https://example.invalid/widget/releases",
        fallback_archive: "widget-update",
        payload: &["widget", "lib", "runtime"],
        bundle_executable: "Contents/MacOS/widget",
        windows_arp_key: r"Software\Widget\NeverWritten_is1",
        must_defer: || false,
    };

    #[gpui::test]
    fn the_identity_comes_back_the_way_it_went_in(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(FAKE, cx);
            let app = identity(cx);
            assert_eq!(app.name, "widget");
            assert_eq!(app.version, "0.2.0");
            assert_eq!(app.payload, ["widget", "lib", "runtime"]);
            assert!(!(app.must_defer)());
            // And the background paths see the same one.
            assert_eq!(current_identity().name, "widget");
        });
    }

    /// The whole reason [`init_process_identity`] exists: a host that has to
    /// run [`crate::update::apply_pending`] or [`crate::update::clean_leftovers`]
    /// before it can build an [`App`] needs the process-wide slot filled
    /// without one — unlike every other test in this module, this one builds
    /// no `App` at all, which is the point.
    #[test]
    fn the_process_identity_can_be_installed_with_no_app_around() {
        init_process_identity(FAKE);
        assert_eq!(current_identity().name, "widget");
        assert_eq!(current_identity().version, "0.2.0");
        // Calling it again with the same identity is a no-op, not a panic.
        init_process_identity(FAKE);
        assert_eq!(current_identity().name, "widget");
    }

    #[gpui::test]
    fn a_lookup_translates_and_interpolates(cx: &mut TestAppContext) {
        cx.update(|cx| {
            set_strings(Box::new(Words), cx);
            assert_eq!(label(cx, "common.close"), "Close");
            assert_eq!(
                text(
                    cx,
                    "update.available",
                    &[("app", "widget"), ("version", "0.3.0")]
                ),
                "widget 0.3.0 is available."
            );
        });
    }

    #[gpui::test]
    fn a_key_nobody_translated_reaches_the_screen_rather_than_panicking(cx: &mut TestAppContext) {
        cx.update(|cx| {
            // Before anything is installed at all: still no panic inside a
            // render pass, which is the whole point.
            assert_eq!(label(cx, "common.close"), "common.close");
            set_strings(Box::new(Words), cx);
            assert_eq!(label(cx, "nothing.here"), "nothing.here");
        });
    }

    #[gpui::test]
    fn the_ignored_tag_goes_through_the_host_and_comes_back(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let held = std::sync::Arc::new(std::sync::Mutex::new(None));
            // With no policy installed, nothing is remembered and nothing
            // fails: an application that offers no "ignore this version" simply
            // never suppresses anything.
            assert_eq!(ignored_release(cx), None);
            set_ignored_release(Some("v9.9.9".to_string()), cx);

            set_update_policy(Box::new(Policy(held.clone())), cx);
            assert_eq!(ignored_release(cx), None);
            set_ignored_release(Some("v9.9.9".to_string()), cx);
            assert_eq!(ignored_release(cx), Some("v9.9.9".to_string()));
            // The policy is still installed afterwards — it is taken out for
            // the call and put back.
            set_ignored_release(None, cx);
            assert_eq!(ignored_release(cx), None);
            assert_eq!(*held.lock().expect("no panic while held"), None);
        });
    }
}
