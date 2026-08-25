//! The two start-up calls, made by a host that wired nothing up.
//!
//! `ruui_shell::update::apply_pending` and
//! `ruui_shell::update::clean_leftovers` are the two functions a host is asked
//! to call before it has a `gpui::App` — which makes them the two it is
//! likeliest to reach with no identity installed, out of an ordering mistake in
//! `main`. Neither may panic: an application that starts is worth more than a
//! staged update, and the correct answer to "there is no identity" is a line in
//! the log and an ordinary launch.
//!
//! This lives in a test binary of its own rather than beside the unit tests
//! because the identity is a *process* global. A unit test would have to clear
//! it, and every other test in that binary — they run on threads of one
//! process — installs it and reads it back. Here nothing installs one at all,
//! which is the condition under test, and it holds for the whole run.

/// Neither call may take the process down, and neither may do anything.
///
/// `apply_pending` answers `false`, which means "carry on starting up
/// normally", and `clean_leftovers` removes nothing — there is no payload to
/// know the names of.
#[test]
fn the_start_up_calls_answer_rather_than_panic_with_no_identity_installed() {
    assert!(
        !ruui_shell::update::apply_pending(),
        "with no identity there is nothing to apply and nothing to restart into"
    );
    ruui_shell::update::clean_leftovers();

    // And still nothing: neither call installs an identity as a side effect,
    // so a second one takes the same path.
    assert!(!ruui_shell::update::apply_pending());
    ruui_shell::update::clean_leftovers();

    // Nothing was recorded either, which is what `set_restart_path` wants: the
    // shell answers `None` rather than a path it never read.
    assert_eq!(ruui_shell::restart_path(), None);
}
