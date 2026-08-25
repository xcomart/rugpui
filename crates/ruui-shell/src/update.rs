//! The update check, and the self-update it leads to.
//!
//! Two halves that share one HTTPS client and one notion of what a release is.
//!
//! The **check** is one request against the host's `releases/latest` endpoint —
//! [`AppIdentity::latest_release_api`]. It runs once per launch from the
//! background executor, and it runs again — with a different filter — whenever
//! the user picks "Check for updates" from a menu. Its whole visible outcome is
//! the [`UpdateDialog`](crate::UpdateDialog) appearing.
//!
//! The **install** is what "Update" does: fetch the release asset built for
//! this exact target triple, verify it against what the API said it should be,
//! unpack it beside the installed copy, and move the new one into the old one's
//! place. The application then restarts itself into the build it just wrote.
//!
//! # What the host supplies
//!
//! Everything specific to one application is in [`AppIdentity`], installed once
//! with [`crate::init`]: the endpoint and the releases page, the name a release
//! asset is published under, the payload a release archive carries, the
//! uninstall key a Windows installer wrote, and the one question only the
//! application can answer — [`AppIdentity::must_defer`]. Nothing here names an
//! application, and nothing here may come to.
//!
//! # Why the start-up check fails silently
//!
//! A workbench is opened to get work done, and an update check is the least
//! important thing happening at start-up. Every way it can go wrong — no
//! network, a captive portal answering HTML, GitHub rate-limiting the address,
//! a tag someone pushed by hand in a shape the parser does not recognise — has
//! the same correct response: say nothing and carry on. So [`check`] ends every
//! failure path in a `log::debug!` and a `None`.
//!
//! A *manual* check is the opposite: the user asked a question and is owed an
//! answer, including "I could not reach GitHub". That is why [`check_now`]
//! answers with a three-way [`Check`] instead, and why the manual path also
//! ignores the "never mention this version again" tag — the user has just
//! overruled it by asking.
//!
//! # Why `ureq` and not gpui's HTTP client
//!
//! `cx.http_client()` is a `NullHttpClient` unless the application installs a
//! real one, and none of the applications sharing this shell does. `ureq` is
//! the smallest client that speaks TLS, and the whole cost of the two requests
//! this module makes is the `gzip` feature the GitHub API's compressed JSON
//! needs.
//!
//! # Why the swap moves the whole payload and not one file
//!
//! An application is not always a single file. One that resolves a bundled
//! runtime, a JAR or a library directory *relative to itself* would, after a
//! swap of the executable alone, be a new binary beside old companions — a
//! mismatch that only shows up the first time one of them is loaded. So
//! [`AppIdentity::payload`] names every entry that has to move, they are
//! replaced together, and if any one of them cannot be moved the ones already
//! moved are put back: a half-swapped installation is worse than no swap at
//! all. A single-file application names one entry and the journal below costs
//! it nothing. On macOS the payload is the one application bundle, which holds
//! all of it, and the problem does not arise.
//!
//! What a release archive carries *besides* the payload is deliberately not
//! swapped — a Linux archive's `icons/`, `.desktop` file and `install.sh`, for
//! instance. Nothing resolves those relative to the executable: the installer
//! copied them into `~/.local/share/applications` and `~/.local/share/icons`,
//! so replacing the copies inside the application directory would update files
//! nobody reads while leaving the installed ones alone. Desktop integration is
//! the installer's business, not the updater's.
//!
//! # Why a successful update also writes one registry value
//!
//! Windows ships twice. The zip is what this module downloads; beside it goes
//! an Inno Setup installer, which exists so the Windows Package Manager has
//! something it can install and account for. What the installer adds is not
//! files — it lays down the same tree — but an entry under *Apps & features*,
//! and winget reads that entry's `DisplayVersion` to decide which version is
//! present and whether an upgrade is available. The updater replaces the files
//! and would otherwise know nothing about it, so an installed copy that updated
//! itself would leave winget convinced the old release was still there:
//! `winget list` reporting a version that has not been on disk for months, and
//! `winget upgrade` offering — and then pointlessly reinstalling — a release
//! already applied. One value, written once per update, is the whole fix.
//!
//! [`sync_arp_version`] is written to be a no-op everywhere it is not wanted,
//! and the two things it refuses to do are the interesting ones.
//!
//! **It never creates the key.** A copy unpacked from the portable archive has
//! no entry, is not an installed program, and inventing one would put the
//! application in a list whose only offered action — uninstall — would run an
//! uninstaller that is not there.
//!
//! **It writes only to an entry that describes *this* copy.** The two
//! distributions can sit on one machine at once: an installed copy under
//! `%LOCALAPPDATA%\Programs\<name>` and a zip unpacked wherever the user keeps
//! it. If the portable copy updated itself and bumped the installed copy's
//! recorded version, the installed copy would drop out of winget's upgrade list
//! while its files stayed at the old release — a worse failure than the one
//! being fixed, because nothing afterwards corrects it. So the entry's
//! `InstallLocation` is compared against the directory this executable is
//! actually running from, and a mismatch means the entry belongs to someone
//! else and is left alone.
//!
//! # A loaded runtime is a one-way door
//!
//! Some applications load a runtime into their own process and cannot unload it
//! again — a JVM through JNI is the case this was written for: it stays until
//! the process exits, and while it is up Windows holds open handles on the JAR
//! and on the loaded images under the bundled runtime, so renaming either of
//! them fails with a sharing violation.
//!
//! The main path is unaffected — the start-up announcement arrives long before
//! anything of the kind is loaded, so the swap runs against a process that has
//! never touched it. Updating from a menu *after* it is loaded is the case that
//! cannot rename anything, and rather than fail there, [`install`] parks the
//! unpacked payload beside the installation as [`PENDING_DIR`] and reports
//! [`Installed::Staged`]. The next launch finds it and performs the same
//! renames from [`apply_pending`], before a window, a settings load or a
//! connection exists — a moment at which nothing has been loaded and the only
//! locked file is the running executable, which Windows *does* allow to be
//! renamed. The user sees one flow either way: the dialog finishes and the
//! application comes back up on the new build.
//!
//! Three decisions inside that are worth writing down.
//!
//! **The fallback is chosen up front, not after a failure.** The question is
//! [`AppIdentity::must_defer`], asked before the first rename — see
//! [`must_defer`]. Trying the swap and staging on failure sounds more general
//! and is worse: Windows reports a sharing violation and a permissions problem
//! alike as `ERROR_ACCESS_DENIED`, so an installation directory the user cannot
//! write would be staged, and staged again, forever, instead of saying so. A
//! question with a knowable answer is both deterministic and testable.
//!
//! **Only where a rename would actually fail.** Elsewhere a rename over a
//! running image succeeds, so a swap that fails there failed for a reason the
//! next launch will not change — a system package, a read-only mount, a `.app`
//! opened from a disk image. Deferring those would trade today's honest error
//! dialog, which names the problem and offers the release page, for a silent
//! success that quietly does nothing on the next launch. That is why the host's
//! answer is expected to name the platform as well as the condition.
//!
//! **A staged update that cannot be applied is discarded, quietly.** If the
//! pending directory turns out to be incomplete, or the swap fails anyway,
//! [`apply_pending`] logs a warning, removes the directory and lets the
//! installed build start normally. This is the fallback's own fallback, reached
//! where there is no window to put a dialog in; what the user needs at start-up
//! is a working application, and keeping the directory would only turn one
//! failure into a failure on every launch — or, worse, apply a stale payload as
//! a downgrade months later.
//!
//! # What the install deliberately does not do
//!
//! No package manager is consulted, no installer is run, nothing is elevated,
//! and the only thing written outside the directory the application is already
//! installed in is the single `DisplayVersion` value above — a correction to a
//! record of that same directory, not a claim on anything else. A copy the user
//! cannot overwrite — a system package, a read-only mount, a `.app` opened from
//! a disk image — fails the rename and lands in the dialog's error state, whose
//! one action is the browser fallback. That is the honest outcome: an updater
//! that starts asking for administrator rights is a different program.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::App;

use crate::inject::{self};

/// Whether the start-up check is allowed to make a request at all.
///
/// The one knob the host has over this module's behaviour, and it exists for
/// test suites. gpui's test executor runs background tasks inline whenever a
/// test parks, so a workspace built dozens of times in a suite would make
/// dozens of real requests to github.com — a test run that needs the network,
/// and that pays [`TIMEOUT`] per test without it. A host whose tests build
/// windows turns this off, or guards its own call site with `cfg!(test)`, which
/// is the same thing said in the host's crate where `cfg(test)` actually
/// applies.
///
/// The manual [`check_now`] is deliberately untouched by it: nothing starts one
/// except a user picking a menu item.
static STARTUP_CHECK: AtomicBool = AtomicBool::new(true);

/// What every path here says when the host wired nothing up.
///
/// [`apply_pending`] and [`clean_leftovers`] are the two functions a host is
/// asked to call before it has an [`App`], which makes them the two it is
/// likeliest to reach with no identity installed. Panicking there would take
/// down a launch over a mis-ordered start-up; the honest answer is a line in
/// the log saying which call is missing, and a start-up that carries on.
const NO_IDENTITY: &str = "identity not installed — call init_process_identity first";

/// How long the whole *check* may take, connection included.
///
/// Short on purpose. Nothing waits on this — the window is already up — but a
/// background task blocked for minutes on a black-holed connection is a thread
/// of the executor pool held hostage for no possible benefit, and an answer
/// that arrives long after start-up would open a dialog over whatever the user
/// had started doing in the meantime.
///
/// Emphatically *not* reused for the download: see [`CONNECT_TIMEOUT`].
const TIMEOUT: Duration = Duration::from_secs(5);

/// How long the *download* may take to reach the server.
///
/// A global timeout is wrong for a download — a release archive on a slow line
/// legitimately takes minutes, and an archive carrying a bundled runtime is
/// measured in tens of megabytes rather than in one. Killing it at any fixed
/// deadline would make the updater useless exactly where it is most wanted.
/// What can still be bounded is the handshake, so an unreachable host fails
/// quickly instead of leaving the dialog spinning at 0%.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Ceiling on what the download will write to disk.
///
/// The size the API reported is checked afterwards, but only a reader that
/// stops can do the checking; without a limit a server answering an endless
/// body would fill the volume first. An order of magnitude above any release
/// this project has published — the bundled runtime is most of one — so it can
/// only ever catch a fault.
const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;

/// Copy buffer for the download.
const DOWNLOAD_BUFFER: usize = 64 * 1024;

/// How many bytes must land before the download reports progress again.
///
/// The read loop turns over hundreds of times a second; a report per turn would
/// wake the UI thread for a bar that has not moved a pixel.
const PROGRESS_STEP: u64 = 256 * 1024;

/// Name of the scratch directory the download and the unpacking happen in.
///
/// Created *beside the installed copy* rather than in the system temp
/// directory, and that placement is load-bearing: the last step of an install
/// is a `fs::rename` of the unpacked payload onto the installed one, and a
/// rename cannot cross a volume. Staging in `%TEMP%` or `/tmp` would work on
/// most machines and fail with `EXDEV` on exactly the ones where the
/// application lives on another disk.
const STAGING_DIR: &str = ".update";

/// Where the unpacked archive goes inside [`STAGING_DIR`].
const UNPACKED_DIR: &str = "unpacked";

/// Name of the directory a deferred update waits in until the next launch.
///
/// A sibling of the installed copy for the same reason [`STAGING_DIR`] is — the
/// swap that eventually consumes it is a `fs::rename`, which cannot cross a
/// volume — but deliberately *not* inside it: [`install`] deletes the staging
/// directory on its way out, and a payload parked in there would go with it.
const PENDING_DIR: &str = ".update-pending";

/// Suffix a replaced entry is renamed to.
///
/// Windows will not let a running executable be deleted, but it will let it be
/// renamed, which is what makes an in-place swap possible at all. The leftovers
/// are removed by [`clean_leftovers`] on the next launch — one code path for all
/// three platforms, rather than an immediate unlink on unix and a deferred one
/// on Windows.
const OLD_SUFFIX: &str = ".old";

/// The value inside the uninstall key that winget reads as the installed
/// version.
///
/// The key itself is [`AppIdentity::windows_arp_key`]: it carries the `AppId`
/// GUID an Inno Setup installer derived it from, which is a published
/// identifier of the *application* and so is not this crate's to name. See the
/// module docs for what happens when the two disagree.
#[cfg(windows)]
const DISPLAY_VERSION: &str = "DisplayVersion";

/// The value inside that key naming the directory the entry describes.
#[cfg(windows)]
const INSTALL_LOCATION: &str = "InstallLocation";

/// `CREATE_NO_WINDOW`, so the `tar` this module shells out to does not flash a
/// console window over the progress dialog.
///
/// Spelled out rather than taken from the `windows` crate: that dependency is
/// scoped to [`crate::caption`] and pulling a whole feature of it in for one
/// constant would be the larger change.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The release-asset target triple for the platform this binary was built for,
/// or `None` where the project publishes no build.
///
/// The three arms are the three builds the applications sharing this shell all
/// publish, and they are the same three in each of their release workflows.
/// An Intel Mac or an ARM Linux box runs a locally built copy, and there is
/// nothing to hand it: those fall through to `None`, which makes "Update" open
/// the release page instead.
const TARGET: Option<&str> = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("x86_64-pc-windows-msvc")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("aarch64-apple-darwin")
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("x86_64-unknown-linux-gnu")
} else {
    None
};

/// One thing the swap has to move: where it goes, and what goes there.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// The installed path being replaced. It need not exist — a development
    /// tree has no `runtime/` beside the binary — in which case there is
    /// nothing to move aside and the new copy simply arrives.
    target: PathBuf,
    /// Name of the matching entry inside the unpacked payload directory.
    source: &'static str,
}

/// What one completed [`Entry`] left behind, so it can be undone.
#[derive(Debug)]
struct Done {
    /// Where the new copy now is.
    target: PathBuf,
    /// Where the displaced copy went, or `None` when there was nothing there.
    retired: Option<PathBuf>,
    /// Where in the payload directory the new copy came from, and where a
    /// rollback puts it back.
    source: PathBuf,
}

/// A downloadable build of a release, matched to this target triple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// File name as published, e.g.
    /// `<name>-v0.2.0-x86_64-pc-windows-msvc.zip`.
    pub name: String,
    /// Direct download URL. Answers a redirect to storage, which `ureq`
    /// follows.
    pub url: String,
    /// Size in bytes, as the API reported it. Checked against what actually
    /// arrived, and used to drive the progress bar.
    pub size: u64,
    /// Lower-case hex SHA-256 of the asset, when the API supplied one.
    ///
    /// `digest` is a recent addition to the releases API, so an older GitHub
    /// Enterprise or a cached response may omit it; the size check still
    /// applies in that case.
    pub digest: Option<String>,
}

/// A release worth telling the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The git tag GitHub published it under, e.g. `"v0.2.0"`.
    ///
    /// Kept verbatim rather than normalised, because this is also what gets
    /// written to `settings.json` when the user ignores the version, and the two
    /// are compared as strings.
    pub tag: String,
    /// Human-readable version for display: [`Release::tag`] without its `v`.
    pub version: String,
    /// The release page to open in the browser.
    pub url: String,
    /// The build for this platform, when the release published one.
    ///
    /// `None` on a target the project does not ship — and on any release whose
    /// assets do not include the expected name — which is what decides whether
    /// "Update" installs or hands off to the browser.
    pub asset: Option<Asset>,
}

/// The answer to a check the user asked for.
///
/// Distinguishes the two outcomes the start-up check collapses into `None`:
/// "there is nothing newer" is a satisfying answer to a question, and "GitHub
/// could not be reached" is not the same thing at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// A newer release exists.
    Newer(Release),
    /// The running build is the latest one published.
    UpToDate,
    /// The check itself did not complete. Carries a short technical detail —
    /// untranslated on purpose, see [`install`].
    Failed(String),
}

/// How an [`install`] ended, both of which are successes.
///
/// The distinction is for the log and nothing else: the caller restarts either
/// way, and the user is told the same thing. See the module docs for when the
/// second one happens and why it is not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// The new build is in place. A restart comes up on it.
    Swapped,
    /// The new build is unpacked and waiting in [`PENDING_DIR`]. A restart
    /// applies it from [`apply_pending`] and re-executes once more, which is
    /// still one visible restart.
    Staged,
}

/// How far an [`install`] has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// `done` of `total` bytes have been written to the staging directory.
    Downloading {
        /// Bytes received so far.
        done: u64,
        /// Bytes the API said the asset has. Zero when it said nothing.
        total: u64,
    },
    /// The download is complete; the archive is being unpacked and swapped in.
    Installing,
}

/// Ask GitHub whether a newer release exists, blocking until it answers.
///
/// **Call this from the background executor.** It performs a network request
/// and will block the calling thread for up to [`TIMEOUT`].
///
/// This is the *manual* check: it reports every outcome, and it knows nothing
/// about the ignore list. [`check`] is the start-up wrapper around it.
pub fn check_now() -> Check {
    let body = match fetch_latest() {
        Ok(body) => body,
        Err(err) => {
            log::debug!("update check: {err}");
            return Check::Failed(err.to_string());
        }
    };

    let Some(release) = parse_release(&body) else {
        return Check::Failed("GitHub answered with no readable release".to_string());
    };

    let current = inject::current_identity().version;
    if is_newer(&release.tag, current) {
        Check::Newer(release)
    } else {
        log::debug!("update check: {} is not newer than {current}", release.tag);
        Check::UpToDate
    }
}

/// The start-up check: answers `Some` only when there is something to say.
///
/// Answers `Some` only when all of the following hold, and `None` — silently —
/// otherwise:
///
/// * the request succeeded and the body parsed;
/// * the tag names a version strictly newer than the running one;
/// * that tag is not the one stored in `ignored`.
///
/// `ignored` is passed in rather than read from the settings global because the
/// global is only reachable from the UI thread, and this function runs off it.
///
/// Answers `None` immediately, without a request, when the host has turned the
/// start-up check off with [`set_startup_check_enabled`]; see [`STARTUP_CHECK`].
pub fn check(ignored: Option<&str>) -> Option<Release> {
    if !STARTUP_CHECK.load(Ordering::Relaxed) {
        return None;
    }
    match check_now() {
        Check::Newer(release) if ignored == Some(release.tag.as_str()) => {
            log::debug!("update check: {} is available but ignored", release.tag);
            None
        }
        Check::Newer(release) => Some(release),
        Check::UpToDate | Check::Failed(_) => None,
    }
}

/// The release page of `release`, or the releases index when it named none.
pub fn release_url(release: &Release) -> &str {
    if release.url.is_empty() {
        inject::current_identity().releases_page
    } else {
        &release.url
    }
}

/// Whether the start-up check may make a request.
///
/// See [`STARTUP_CHECK`] for why a host would ever say no.
pub fn set_startup_check_enabled(enabled: bool) {
    STARTUP_CHECK.store(enabled, Ordering::Relaxed);
}

/// The release tag the user has asked never to be told about again.
///
/// Read through the host's [`UpdatePolicy`](crate::UpdatePolicy), because the
/// tag lives in the host's settings file. Hand the answer to [`check`], which
/// runs off the UI thread and so cannot read it itself.
pub fn ignored_release(cx: &App) -> Option<String> {
    inject::ignored_release(cx)
}

/// Persist `tag` as the version the user never wants to hear about again.
///
/// Handed straight to the host's [`UpdatePolicy`](crate::UpdatePolicy), which
/// is expected to write the file immediately rather than at the next save: this
/// is a decision the user just made in a dialog, not a window position drifting
/// under a drag, and it has to survive a crash the same way a saved setting
/// does.
pub fn remember_ignored(tag: &str, cx: &mut App) {
    inject::set_ignored_release(Some(tag.to_string()), cx);
}

/// Remove what a previous update left behind, if anything.
///
/// **Call this from the background executor**, early in the run: removing a
/// `.app` bundle or a bundled JRE is a recursive delete of thousands of files,
/// and nothing on screen depends on it.
///
/// The swap cannot delete the copies it replaces — on Windows because one of
/// them is the running process, on the others because there is no reason to
/// make the three platforms differ — so it renames them aside and leaves them
/// for the next launch. That is here. Every failure is a debug line: a leftover
/// costs disk space and nothing else, and the next update will try again.
///
/// # Order
///
/// [`crate::init`] or [`crate::init_process_identity`] first: this reads the
/// payload out of the identity, and with none installed it logs an error and
/// removes nothing. It does not panic — a start-up housekeeping sweep is not
/// worth a launch over.
pub fn clean_leftovers() {
    let Ok(plan) = install_plan() else {
        return;
    };
    for entry in plan {
        let Some(retired) = old_path(&entry.target) else {
            continue;
        };
        if !retired.exists() {
            continue;
        }
        match remove(&retired) {
            Ok(()) => log::debug!("removed the previous version at {}", retired.display()),
            Err(error) => log::debug!(
                "could not remove the previous version at {}: {error}",
                retired.display()
            ),
        }
    }
}

/// Download `release`, unpack it, and put it where the running copy is.
///
/// **Call this from the background executor.** It downloads tens of megabytes,
/// spawns `tar`, and renames files; none of that belongs on the UI thread.
/// `report` is called as the work proceeds, from this thread.
///
/// Returns [`Installed::Swapped`] only once the new build is fully in place, so
/// the caller may restart into it immediately, and [`Installed::Staged`] when
/// the swap had to be left to the next launch — see the module docs. Both are
/// successes and both are followed by a restart. On failure the staging
/// directory is gone, the installed copy is as it was, and the `Err` carries a
/// sentence for the dialog to show under its translated "the update failed"
/// heading.
///
/// # Why the error text is not translated
///
/// It is a technical detail — a `tar` message, an OS error, a byte count that
/// did not match — produced on a thread that has no business reaching into the
/// locale state, and shown beneath a heading that *is* translated. Translating
/// the detail would mean a key per failure mode and a per-locale copy of every
/// `io::Error` string, which is not what any of them say anyway.
pub fn install(release: &Release, report: &mut dyn FnMut(Progress)) -> Result<Installed, String> {
    let Some(asset) = release.asset.as_ref() else {
        return Err(format!(
            "{} publishes no build for this platform",
            release.tag
        ));
    };

    let plan = install_plan()?;
    let parent = install_dir(&plan)
        .ok_or_else(|| "the installed copy has no parent directory".to_string())?
        .to_path_buf();

    let staging = parent.join(STAGING_DIR);
    // A staging directory left by an interrupted run would otherwise poison
    // this one with a half-written archive under the same name.
    let _ = remove(&staging);
    fs::create_dir_all(&staging)
        .map_err(|error| format!("could not write to {}: {error}", parent.display()))?;

    let outcome = stage(asset, &plan, &staging, &parent.join(PENDING_DIR), report);
    // Best-effort on purpose: the update either happened or it did not, and a
    // scratch directory that outlives it is not worth turning a success into a
    // failure over. The next install removes it anyway.
    let _ = remove(&staging);

    // Both of `stage`'s successes come through here and both are treated alike,
    // and this is also the last place that still knows *which* release was
    // installed — the two facts that settle where the value is written from.
    // See the notes on `sync_arp_version`.
    #[cfg(windows)]
    if outcome.is_ok() {
        sync_arp_version(&parent, &release.version);
    }

    outcome
}

/// The download-verify-unpack-swap sequence, with `staging` already prepared.
///
/// Split out from [`install`] purely so the staging directory has exactly one
/// removal site covering every way out of it.
fn stage(
    asset: &Asset,
    plan: &[Entry],
    staging: &Path,
    pending: &Path,
    report: &mut dyn FnMut(Progress),
) -> Result<Installed, String> {
    let archive = staging.join(archive_name(&asset.name));
    download(asset, &archive, report)?;

    report(Progress::Installing);

    let unpacked = staging.join(UNPACKED_DIR);
    fs::create_dir_all(&unpacked)
        .map_err(|error| format!("could not create {}: {error}", unpacked.display()))?;
    extract(&archive, &unpacked)?;

    let names = inject::current_identity().payload;
    let payload = find_payload(&unpacked, names)
        .ok_or_else(|| format!("{} does not contain {}", asset.name, names.join(" beside ")))?;

    if must_defer() {
        defer(&payload, pending)?;
        log::info!(
            "the update is staged at {} and will be applied on the next launch",
            pending.display()
        );
        return Ok(Installed::Staged);
    }

    swap(plan, &payload)?;

    // With the new bundle in place and the restart imminent, this is the last
    // moment to make sure Gatekeeper will let it open.
    #[cfg(target_os = "macos")]
    if let Some(entry) = plan.first() {
        clear_quarantine(&entry.target);
    }

    Ok(Installed::Swapped)
}

/// Strip the quarantine flag from the bundle just swapped in, best-effort.
///
/// A file this process downloads and unpacks should carry no quarantine of its
/// own — nothing here is quarantine-aware, and `tar` restores none from the
/// CI-built archive — but Gatekeeper's rules have tightened release by release,
/// and the one unacceptable outcome here is an update that leaves the user with
/// an app macOS refuses to reopen. So the flag is cleared unconditionally: this
/// is the same `xattr -r -d com.apple.quarantine` a first-time installer is
/// usually walked through, recursive because the flag lands on every file inside a
/// quarantined bundle, and best-effort because the attribute is usually not
/// there at all — a failure costs a debug line, never the update.
#[cfg(target_os = "macos")]
fn clear_quarantine(bundle: &Path) {
    match Command::new("xattr")
        .args(["-r", "-d", "com.apple.quarantine"])
        .arg(bundle)
        .output()
    {
        Ok(output) if output.status.success() => {}
        // The usual answer on a clean bundle: "No such xattr". Worth a debug
        // line and nothing more.
        Ok(output) => log::debug!(
            "xattr -r -d com.apple.quarantine {} exited with {}: {}",
            bundle.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => log::debug!("xattr could not be run: {error}"),
    }
}

/// Tell the "Apps & features" entry for this installation that `version` is now
/// what is on disk.
///
/// `installed_at` is the directory the running executable lives in — the same
/// one the swap has just written into, or is about to. Called once from
/// [`install`], on both of its successes; see the module docs for what the value
/// is for and why an entry describing some other directory is left alone.
///
/// Answers nothing, and fails at nothing. Every way this can go wrong — no
/// entry, an entry for a different copy, a machine-wide entry this unelevated
/// process may read but not write — ends in a `log::debug!` and a return,
/// because by the time it runs the update itself has already succeeded. An
/// updater that reported failure over a registry value winget reads would be
/// telling the user their update did not happen, which is both wrong and
/// unactionable.
///
/// `HKEY_CURRENT_USER` is tried first because that is where the installer's
/// `PrivilegesRequired=lowest` puts the entry, and `HKEY_LOCAL_MACHINE` after
/// it, for the copy someone installed by running the setup elevated. Both are
/// tried even when the first one exists: a machine can carry one of each, and
/// only one of them can be the copy running this code.
///
/// # Why a staged update writes it now rather than when it lands
///
/// [`Installed::Staged`] has not moved a file yet — the renames happen on the
/// next launch, from [`apply_pending`]. Writing the value there would follow
/// the order of events more literally and is not possible: all that survives
/// into the next process is a directory of unpacked files, and nothing in it
/// says which release it came from. The version string exists here and nowhere
/// else.
///
/// So it is written at the point where the payload is downloaded, verified and
/// parked beside the installation — where the update has stopped being a plan
/// and become a certainty. What that gives up is the rare path where
/// [`apply_pending`] discards a payload it cannot apply: the entry then reads
/// one release ahead of the files until the user updates again. That is the
/// same shape of wrongness this function exists to remove, one release smaller,
/// self-correcting, and confined to a path already understood as a failure —
/// against the alternative of never writing the value at all for any user who
/// has opened a connection before updating, which on Windows is the common way
/// to reach this code.
#[cfg(windows)]
fn sync_arp_version(installed_at: &Path, version: &str) {
    let key_path = inject::current_identity().windows_arp_key;
    let roots = [
        ("HKCU", windows_registry::CURRENT_USER),
        ("HKLM", windows_registry::LOCAL_MACHINE),
    ];
    for (name, root) in roots {
        if write_display_version(root, name, key_path, installed_at, version) {
            return;
        }
    }
}

/// The body of [`sync_arp_version`], for one registry root and one key path.
///
/// Split out with the key path as an argument for one reason: the real key is a
/// live part of the machine's installed-program list, and a test that wrote to
/// it would be editing the user's *Apps & features*. Everything with a decision
/// in it is therefore here, reachable with a scratch key under
/// `HKCU\Software`, and [`sync_arp_version`] is the four lines that supply the
/// constants.
///
/// `name` is the root's short name, for the log lines and nothing else. Answers
/// whether the value was written, which is how the caller knows to stop.
#[cfg(windows)]
fn write_display_version(
    root: &windows_registry::Key,
    name: &str,
    key_path: &str,
    installed_at: &Path,
    version: &str,
) -> bool {
    // Opened for writing up front, so a machine-wide entry an unelevated
    // process cannot touch fails here rather than after the comparison. The
    // overwhelmingly common outcome is the key simply not being there, which is
    // what a copy unpacked from the zip looks like.
    let key = match root.options().read().write().open(key_path) {
        Ok(key) => key,
        Err(error) => {
            log::debug!("{name}\\{key_path} is not open for writing: {error}");
            return false;
        }
    };

    let recorded = match key.get_string(INSTALL_LOCATION) {
        Ok(recorded) => recorded,
        Err(error) => {
            // An entry with no `InstallLocation` describes no directory, so
            // there is nothing to match it against and no basis for claiming it.
            log::debug!("{name}\\{key_path} records no {INSTALL_LOCATION}: {error}");
            return false;
        }
    };

    if !same_directory(Path::new(recorded.trim()), installed_at) {
        log::debug!(
            "{name}\\{key_path} describes {recorded}, not {}; its version is not this copy's to \
             change",
            installed_at.display()
        );
        return false;
    }

    match key.set_string(DISPLAY_VERSION, version) {
        Ok(()) => {
            log::debug!("{name}\\{key_path} now reports {version} as the installed version");
            true
        }
        Err(error) => {
            log::debug!("could not write {DISPLAY_VERSION} to {name}\\{key_path}: {error}");
            false
        }
    }
}

/// Whether two paths name the same directory.
///
/// Both sides go through `canonicalize` rather than being compared as text,
/// because they are written by different programs and agree on nothing else:
/// Inno Setup stores `InstallLocation` with a trailing backslash, the two may
/// differ in case on a filesystem that does not care, and either may run
/// through a junction or a substituted drive. Canonicalising resolves all of
/// that to the one form the operating system itself would.
///
/// A path that does not exist cannot be canonicalised, and the answer there is
/// `false`: a recorded install location pointing at a directory that is gone
/// describes some other, broken installation, and is emphatically not this one.
#[cfg(any(windows, test))]
fn same_directory(one: &Path, other: &Path) -> bool {
    match (fs::canonicalize(one), fs::canonicalize(other)) {
        (Ok(one), Ok(other)) => one == other,
        _ => false,
    }
}

/// Whether the renames have to be left to the next launch.
///
/// The host's own answer, which is [`AppIdentity::must_defer`]. See the module
/// docs for why the question is asked up front rather than inferred from a
/// failed rename, and why an application that loads no JVM answers `false`.
fn must_defer() -> bool {
    (inject::current_identity().must_defer)()
}

/// Park the unpacked `payload` at `pending`, for the next launch to apply.
///
/// One rename, of a directory that is still inside the staging tree, onto a
/// sibling of the installed copy — so it survives the staging directory being
/// deleted and stays on the volume the eventual swap has to rename within. It
/// works equally for the wrapper directory every published archive carries and
/// for the unpacked root itself, which is what a flat archive resolves to.
///
/// Any earlier pending directory goes first. It can only be one of two things:
/// a payload this launch already failed to apply, or one staged and then
/// superseded before a restart happened. Neither is worth keeping over the copy
/// that was just downloaded and verified.
fn defer(payload: &Path, pending: &Path) -> Result<(), String> {
    let _ = remove(pending);
    fs::rename(payload, pending).map_err(|error| {
        format!(
            "could not stage the update at {}: {error}",
            pending.display()
        )
    })
}

/// Apply an update a previous run staged, and re-execute into it.
///
/// **Call this first thing in `main`**, before the gpui application exists and
/// long before anything can load the JVM: the whole point is to do the renames
/// in a process that holds no handle on `lib/` or `runtime/`.
///
/// Answers `true` when the caller should return from `main` immediately — the
/// new build is in place and a fresh process carrying this one's arguments has
/// been spawned into it. Answers `false` for every other case, including all the
/// failures, which means "carry on starting up normally"; there is no pending
/// directory left either way, so the next launch is an ordinary one and this can
/// never loop.
///
/// # Order
///
/// [`crate::init_process_identity`] has to have run — it is the one piece of
/// wiring that does not need an [`App`], and it exists for exactly this call.
/// With no identity installed this logs an error and answers `false`, which
/// starts the application on the build already on disk rather than taking the
/// launch down over a mis-ordered `main`.
pub fn apply_pending() -> bool {
    let Ok(plan) = install_plan() else {
        return false;
    };
    let Some(pending) = install_dir(&plan).map(|parent| parent.join(PENDING_DIR)) else {
        return false;
    };
    // The overwhelmingly common case, and the only one that costs anything at
    // start-up: one `stat` that says there is nothing to do.
    if !pending.is_dir() {
        return false;
    }

    if !apply(&plan, &pending) {
        return false;
    }

    let Some(exe) = relaunch_target(&plan) else {
        log::warn!("the staged update was applied but there is nothing to restart");
        return false;
    };

    match Command::new(&exe).args(std::env::args_os().skip(1)).spawn() {
        Ok(_) => true,
        Err(error) => {
            // Vanishingly unlikely — the file was just renamed into place — and
            // there is nothing better to do than carry on. The build on disk is
            // now wholly the new one, so the running image is the only stale
            // part, and it is replaced by the next launch.
            log::warn!(
                "could not restart into the update at {}: {error}",
                exe.display()
            );
            false
        }
    }
}

/// Verify `pending`, swap it in, and remove it whichever way that went.
///
/// Split from [`apply_pending`] so the part with the decisions in it takes its
/// paths as arguments: `current_exe` and `spawn` are not things a test can hold
/// still. Answers whether the installed copy is now the staged one.
///
/// The directory is removed on every path, and that is deliberate. A successful
/// swap renames the entries out of it and leaves only whatever else the archive
/// carried; a failed one leaves the whole payload. Keeping either would mean a
/// launch that failed once fails identically forever, and a payload left by a
/// version the user has since moved past would eventually be applied as a
/// downgrade.
fn apply(plan: &[Entry], pending: &Path) -> bool {
    // The plan's own source names rather than `PAYLOAD`: they are exactly what
    // the swap below will reach for, and a pending directory missing one of them
    // is a swap that fails halfway.
    let names: Vec<&str> = plan.iter().map(|entry| entry.source).collect();

    let applied = if holds_all(pending, &names) {
        match swap(plan, pending) {
            Ok(()) => {
                log::info!("applied the update staged at {}", pending.display());
                true
            }
            Err(error) => {
                log::warn!("could not apply the staged update: {error}");
                false
            }
        }
    } else {
        log::warn!(
            "the update staged at {} is incomplete and has been discarded",
            pending.display()
        );
        false
    };

    if let Err(error) = remove(pending) {
        log::warn!("could not remove {}: {error}", pending.display());
    }

    applied
}

/// The executable to start once a staged update has been applied.
///
/// Everywhere but macOS the plan's first target is the executable itself. On
/// macOS it is the bundle, so the path inside has to be rebuilt — correctness
/// for a case that in practice never arises, since only Windows ever stages.
fn relaunch_target(plan: &[Entry]) -> Option<PathBuf> {
    let target = plan.first()?.target.clone();

    #[cfg(target_os = "macos")]
    {
        Some(target.join(inject::current_identity().bundle_executable))
    }

    #[cfg(not(target_os = "macos"))]
    {
        Some(target)
    }
}

/// The directory `plan` installs into: the parent of the entry it starts with.
fn install_dir(plan: &[Entry]) -> Option<&Path> {
    plan.first()?.target.parent()
}

/// Stream `asset` into `to`, checking it against what the API promised.
///
/// Uses an agent of its own rather than the check's: that one carries a global
/// five-second deadline, which would abort a release download on any connection
/// slower than a datacentre's. Here only the connect phase is bounded.
fn download(asset: &Asset, to: &Path, report: &mut dyn FnMut(Progress)) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .build()
        .into();

    let mut response = agent
        .get(&asset.url)
        .header("User-Agent", user_agent())
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|error| format!("could not download {}: {error}", asset.name))?;

    let mut body = response
        .body_mut()
        .with_config()
        .limit(MAX_ASSET_BYTES)
        .reader();

    let mut file =
        File::create(to).map_err(|error| format!("could not create {}: {error}", to.display()))?;

    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0u8; DOWNLOAD_BUFFER];
    let mut done = 0u64;
    let mut reported = 0u64;

    loop {
        let read = body
            .read(&mut buffer)
            .map_err(|error| format!("could not download {}: {error}", asset.name))?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        digest.update(chunk);
        file.write_all(chunk)
            .map_err(|error| format!("could not write {}: {error}", to.display()))?;
        done = done.saturating_add(read as u64);
        if done - reported >= PROGRESS_STEP {
            reported = done;
            report(Progress::Downloading {
                done,
                total: asset.size,
            });
        }
    }

    file.flush()
        .map_err(|error| format!("could not write {}: {error}", to.display()))?;
    drop(file);

    report(Progress::Downloading {
        done,
        total: asset.size,
    });

    if asset.size != 0 && done != asset.size {
        return Err(format!(
            "{} is {done} bytes, but the release says {}",
            asset.name, asset.size
        ));
    }

    if let Some(expected) = &asset.digest {
        let actual = hex(digest.finish().as_ref());
        if &actual != expected {
            return Err(format!(
                "{} does not match its published checksum",
                asset.name
            ));
        }
    }

    Ok(())
}

/// Unpack `archive` into `into` using the system `tar`.
///
/// One extractor for three archive formats and three platforms, and no new
/// dependency: `tar` on macOS and Linux is bsdtar or GNU tar, both of which
/// autodetect gzip, and Windows has shipped bsdtar as `System32\tar.exe` since
/// 1803 — which also reads the `.zip` the Windows release is published as,
/// because libarchive sniffs the container rather than trusting the extension.
///
/// `CREATE_NO_WINDOW` on Windows because a GUI process starting a console
/// program flashes a black rectangle on screen otherwise, and here it would
/// flash over a progress dialog.
fn extract(archive: &Path, into: &Path) -> Result<(), String> {
    let mut command = Command::new("tar");
    command.arg("-xf").arg(archive).arg("-C").arg(into);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let output = command
        .output()
        .map_err(|error| format!("could not run tar: {error}"))?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            format!("tar could not unpack {}", archive.display())
        } else {
            format!("tar could not unpack {}: {detail}", archive.display())
        });
    }

    Ok(())
}

/// Move every entry of `plan` out of `payload` and into its installed place.
///
/// Each entry is two renames in the only order that leaves a working entry at
/// every intermediate point: the installed copy is renamed out of the way first
/// — the step Windows permits for a running image but a delete would not — and
/// the new one takes the freed name.
///
/// Across entries the sequence is a journal. Every entry that completed is
/// recorded, and the first one that fails undoes all of them in reverse, which
/// a rename can always do. That matters here in a way it does not for a
/// single-file program: an executable that arrived beside the bridge JAR it was
/// not built against would start and then fail at the first connection, with
/// nothing on screen to say why. Either all three move or none do.
fn swap(plan: &[Entry], payload: &Path) -> Result<(), String> {
    let mut done: Vec<Done> = Vec::new();

    for entry in plan {
        match swap_one(entry, payload) {
            Ok(step) => done.push(step),
            Err(error) => {
                return Err(match roll_back(done) {
                    None => error,
                    // The rollback itself failed, which means the directory is
                    // in a state no further attempt can reason about. Say so:
                    // the browser fallback is the only way out, and the user
                    // needs to know where the pieces are.
                    Some(detail) => format!("{error}; {detail}"),
                });
            }
        }
    }

    Ok(())
}

/// Move one entry into place, leaving the copy it displaced under
/// [`OLD_SUFFIX`].
///
/// A failure here is already undone: if the new copy could not be moved in, the
/// displaced one goes straight back, so this either completes or changes
/// nothing. The caller's journal only has to undo the entries that *succeeded*.
fn swap_one(entry: &Entry, payload: &Path) -> Result<Done, String> {
    let source = payload.join(entry.source);

    let retired = if entry.target.exists() {
        let retired = old_path(&entry.target)
            .ok_or_else(|| format!("{} has no file name", entry.target.display()))?;
        // A leftover from a previous update that start-up could not remove
        // would make the rename below fail on Windows, where renaming onto an
        // existing name is an error.
        let _ = remove(&retired);
        fs::rename(&entry.target, &retired)
            .map_err(|error| format!("could not move {} aside: {error}", entry.target.display()))?;
        Some(retired)
    } else {
        // Nothing installed under this name — a development tree with no
        // `runtime/` beside the binary, say. There is nothing to displace and
        // the new copy simply arrives.
        None
    };

    if let Err(error) = fs::rename(&source, &entry.target) {
        let mut message = format!("could not install {}: {error}", entry.target.display());
        if let Some(retired) = &retired
            && let Err(second) = fs::rename(retired, &entry.target)
        {
            message.push_str(&format!(
                "; the previous one is now at {} ({second})",
                retired.display()
            ));
        }
        return Err(message);
    }

    Ok(Done {
        target: entry.target.clone(),
        retired,
        source,
    })
}

/// Undo completed entries, newest first, and report what could not be undone.
///
/// `None` means the installation is exactly as it was before the swap started.
/// Each step frees the installed name — by moving the new copy back into the
/// payload directory, or failing that by deleting it, since the whole staging
/// tree is about to go anyway — and then puts the displaced copy back.
fn roll_back(done: Vec<Done>) -> Option<String> {
    let mut stuck: Vec<String> = Vec::new();

    for step in done.into_iter().rev() {
        let freed = fs::rename(&step.target, &step.source).is_ok() || remove(&step.target).is_ok();
        let Some(retired) = step.retired else {
            // Nothing was displaced, so freeing the name is the whole undo.
            if !freed {
                stuck.push(format!(
                    "{} could not be removed again",
                    step.target.display()
                ));
            }
            continue;
        };
        if !freed {
            stuck.push(format!(
                "{} is the new version and {} is the previous one",
                step.target.display(),
                retired.display()
            ));
            continue;
        }
        if fs::rename(&retired, &step.target).is_err() {
            stuck.push(format!(
                "the previous {} is now at {}",
                step.target.display(),
                retired.display()
            ));
        }
    }

    if stuck.is_empty() {
        None
    } else {
        Some(format!(
            "and the rollback was incomplete: {}",
            stuck.join("; ")
        ))
    }
}

/// Everything this run would replace, in the order it replaces them.
///
/// On macOS that is the one bundle the executable lives inside. Everywhere else
/// it is the executable plus whatever else [`AppIdentity::payload`] names — the
/// directories it resolves relative to itself; see the module docs.
///
/// The macOS arm is the one that can refuse. A `cargo run` build, or a bare
/// binary someone copied out of a bundle, has no `.app` to swap and no sensible
/// thing to do with an archive that contains one, so it reports that rather than
/// scattering a bundle into whatever directory it happens to sit in.
///
/// `current_exe()` resolves symlinks, which is what makes the usual Linux layout
/// work: an `install.sh` puts the tree in `~/.local/share/<name>` and links
/// `~/.local/bin/<name>` at it, and this answers the real directory rather than
/// the link's.
fn install_plan() -> Result<Vec<Entry>, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("could not locate the running program: {error}"))?;
    let Some(app) = inject::try_current_identity() else {
        log::error!("{NO_IDENTITY}");
        return Err(NO_IDENTITY.to_string());
    };
    let payload = app.payload;
    if payload.is_empty() {
        return Err("the application publishes no payload to install".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let bundle = bundle_root(&exe)
            .ok_or_else(|| format!("{} is not running from an application bundle", app.name))?;
        Ok(vec![Entry {
            target: bundle,
            source: payload[0],
        }])
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let parent = exe
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", exe.display()))?
            .to_path_buf();
        // The executable keeps whatever name it is installed under; the
        // companions never differ from the published ones, because the loader
        // looks them up by name.
        let mut plan = vec![Entry {
            target: exe.clone(),
            source: payload[0],
        }];
        plan.extend(payload[1..].iter().map(|name| Entry {
            target: parent.join(name),
            source: name,
        }));
        Ok(plan)
    }
}

/// The `User-Agent` every request from this module carries.
///
/// Not optional politeness: the GitHub API rejects a request without one.
fn user_agent() -> String {
    let app = inject::current_identity();
    format!("{}/{}", app.name, app.version)
}

/// The `.app` directory `exe` lives inside, if any.
///
/// `current_exe()` in a bundle is `<name>.app/Contents/MacOS/<name>`, but the
/// depth is not worth relying on: the ancestor chain is walked until a component
/// wears the `app` extension.
#[cfg(any(target_os = "macos", test))]
fn bundle_root(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .map(Path::to_path_buf)
}

/// `path` with [`OLD_SUFFIX`] appended to its file name.
///
/// Appended to the whole name rather than swapped for the extension, so
/// `widget.exe` becomes `widget.exe.old` and not `widget.old`: the second
/// would collide with a directory listing's idea of a different program, and on
/// Windows it would stop being an executable.
fn old_path(path: &Path) -> Option<PathBuf> {
    let mut name = path.file_name()?.to_os_string();
    name.push(OLD_SUFFIX);
    Some(path.with_file_name(name))
}

/// The directory inside the unpacked archive that holds the whole payload.
///
/// Every published archive wraps its contents in one directory named after the
/// asset, so the payload is one level down — but an archive that ever stops
/// doing that should still install, hence the direct hit is tried first and the
/// immediate subdirectories after it. Nothing deeper: a match further down would
/// be a different tree that happens to share the names.
///
/// A directory qualifies only if it holds *every* name, which is what keeps the
/// Windows and Linux archives from matching on the executable alone and then
/// failing halfway through the swap.
fn find_payload(root: &Path, names: &[&str]) -> Option<PathBuf> {
    if holds_all(root, names) {
        return Some(root.to_path_buf());
    }

    let mut found: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|candidate| candidate.is_dir() && holds_all(candidate, names))
        .collect();
    // Sorted so a two-directory archive picks the same one on every filesystem,
    // rather than whatever order the directory happened to be read in.
    found.sort();
    found.into_iter().next()
}

/// Whether `dir` holds every one of `names`.
///
/// An empty `names` answers `false`: no directory is the payload of an archive
/// that carries nothing, and answering `true` would make the first directory
/// read win.
fn holds_all(dir: &Path, names: &[&str]) -> bool {
    !names.is_empty() && names.iter().all(|name| dir.join(name).exists())
}

/// A file name for the downloaded archive that cannot escape the staging
/// directory.
///
/// The published names are plain, so this returns them unchanged; a name
/// carrying a separator — which only a compromised or confused API could send —
/// is replaced wholesale rather than sanitised, because there is no correct
/// guess at what it was meant to be.
fn archive_name(asset: &str) -> &str {
    let plain = !asset.is_empty()
        && asset != "."
        && asset != ".."
        && !asset.contains('/')
        && !asset.contains('\\');
    if plain {
        asset
    } else {
        inject::current_identity().fallback_archive
    }
}

/// Delete `path`, whichever kind of thing it is.
fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else if path.exists() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

/// Lower-case hex, for comparing against the API's `sha256:` field.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Fetch the raw JSON body of the latest-release endpoint.
///
/// `Accept` pins the response to the current API media type so a future default
/// cannot silently change the field names underneath the parser; see
/// [`user_agent`] for the other header.
fn fetch_latest() -> Result<String, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();

    agent
        .get(inject::current_identity().latest_release_api)
        .header("User-Agent", user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_to_string()
}

/// Pick the tag, the release page and this platform's asset out of a
/// latest-release response.
///
/// `None` when the body is not an object, or carries no usable `tag_name`; a
/// missing `html_url` is tolerated and leaves [`Release::url`] empty, because
/// [`release_url`] has a sensible destination for that case and a release with
/// no page is still worth announcing. A missing asset is tolerated for the same
/// reason, and means the same thing to the dialog: hand off to the browser.
fn parse_release(body: &str) -> Option<Release> {
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(err) => {
            log::debug!("update check: unreadable response: {err}");
            return None;
        }
    };

    let tag = value.get("tag_name")?.as_str()?.trim();
    if tag.is_empty() {
        return None;
    }

    let url = value
        .get("html_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    let asset = TARGET
        .map(|target| asset_name(tag, target))
        .and_then(|name| find_asset(&value, &name));

    Some(Release {
        tag: tag.to_string(),
        version: strip_v(tag).to_string(),
        url: url.to_string(),
        asset,
    })
}

/// The file name the release workflow publishes for `tag` on `target`.
///
/// Mirrors `.github/workflows/release.yml`; the two have to be changed together,
/// and a mismatch degrades to the browser fallback rather than to a wrong
/// download, because nothing in the response would match this name.
fn asset_name(tag: &str, target: &str) -> String {
    let extension = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!(
        "{}-{tag}-{target}.{extension}",
        inject::current_identity().name
    )
}

/// The `assets` entry called `name`, read into an [`Asset`].
///
/// An entry without a download URL is no asset at all, so it answers `None` and
/// the release announces itself without one.
fn find_asset(value: &serde_json::Value, name: &str) -> Option<Asset> {
    let entry = value
        .get("assets")?
        .as_array()?
        .iter()
        .find(|asset| asset.get("name").and_then(serde_json::Value::as_str) == Some(name))?;

    let url = entry
        .get("browser_download_url")
        .and_then(serde_json::Value::as_str)?;
    if url.is_empty() {
        return None;
    }

    Some(Asset {
        name: name.to_string(),
        url: url.to_string(),
        size: entry
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        digest: entry
            .get("digest")
            .and_then(serde_json::Value::as_str)
            .and_then(parse_digest),
    })
}

/// Read the API's `digest` field, which is `"<algorithm>:<hex>"`.
///
/// Only SHA-256 is accepted, and only as exactly 64 hex digits. Anything else —
/// a future algorithm, a truncated value, a field that changed shape — answers
/// `None` and leaves the size check as the only verification, which is the
/// behaviour on the many responses that carry no digest at all.
fn parse_digest(raw: &str) -> Option<String> {
    let (algorithm, hex) = raw.trim().split_once(':')?;
    if !algorithm.eq_ignore_ascii_case("sha256") {
        return None;
    }
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

/// Whether `latest` names a strictly newer version than `current`.
///
/// Both sides are read by [`parse_version`], and anything it cannot read
/// compares as *not* newer. That asymmetry is the point: the only consequence of
/// answering `false` is that a dialog does not appear, while answering `true` on
/// a tag nobody can interpret would nag the user about a release that may not
/// exist. A hand-pushed `nightly` tag, a release named after a branch, an API
/// answering something unexpected — all of them stay quiet.
fn is_newer(latest: &str, current: &str) -> bool {
    let (Some(latest), Some(current)) = (parse_version(latest), parse_version(current)) else {
        return false;
    };

    // Compared position by position rather than as vectors, so that a tag with
    // fewer components than the running version — `v1` against `0.1.2` — is read
    // as `1.0.0` and wins, instead of being cut short by the shorter length.
    let len = latest.len().max(current.len());
    for index in 0..len {
        let left = latest.get(index).copied().unwrap_or(0);
        let right = current.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

/// Split a version string into its numeric components.
///
/// Accepts the `v` prefix the project's tags carry, in either case, and nothing
/// else: every dot-separated component must be a plain non-negative integer.
/// Pre-release and build suffixes (`1.2.3-rc1`, `1.2.3+build`) are therefore
/// rejected rather than truncated — none of the applications sharing this shell
/// publishes one, so a tag wearing one is a surprise, and a surprise should not
/// open a dialog.
fn parse_version(version: &str) -> Option<Vec<u64>> {
    let version = strip_v(version.trim());
    if version.is_empty() {
        return None;
    }
    version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// Drop one leading `v` or `V`, if there is one.
fn strip_v(version: &str) -> &str {
    version
        .strip_prefix('v')
        .or_else(|| version.strip_prefix('V'))
        .unwrap_or(version)
}

#[cfg(test)]
mod tests {
    use crate::AppIdentity;

    use super::*;

    /// A stand-in application, so that no test here names a real one.
    ///
    /// The GUID is a nonce: `sync_arp_version` never creates a key, so the only
    /// thing the tests can assert about one is what happens to a key they made
    /// themselves — which they do, under a scratch path of their own.
    const FAKE: AppIdentity = AppIdentity {
        name: "widget",
        version: "0.2.0",
        repository_url: "https://example.invalid/widget",
        repository_label: "example.invalid/widget",
        latest_release_api: "https://example.invalid/api/releases/latest",
        releases_page: "https://example.invalid/widget/releases",
        fallback_archive: "widget-update",
        payload: &TRIPLE,
        bundle_executable: "Contents/MacOS/widget",
        windows_arp_key: r"Software\Widget\NeverWritten_is1",
        must_defer: || false,
    };

    /// Installs [`FAKE`] and hands it back.
    ///
    /// Every test in this module wants the same identity and the process holds
    /// one, so installing it again is both harmless and the simplest thing that
    /// survives a test runner with threads.
    fn fake() -> AppIdentity {
        inject::init_process_identity(FAKE);
        FAKE
    }

    #[test]
    fn a_higher_component_anywhere_is_newer() {
        assert!(is_newer("0.1.3", "0.1.2"));
        assert!(is_newer("0.2.0", "0.1.2"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.1.3", "0.1.2"));
        assert!(is_newer("V0.1.3", "0.1.2"));
    }

    #[test]
    fn the_same_or_an_older_version_is_not_newer() {
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("v0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.1", "0.1.2"));
        assert!(!is_newer("0.0.9", "0.1.2"));
        assert!(!is_newer("0.1.2", "1.0.0"));
    }

    #[test]
    fn components_compare_numerically_and_not_as_text() {
        // The whole reason not to compare the strings: "10" sorts before "9".
        assert!(is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.9.0", "0.10.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
    }

    #[test]
    fn a_missing_component_counts_as_zero() {
        assert!(!is_newer("0.1", "0.1.2"));
        assert!(!is_newer("0.1.2", "0.1.2.0"));
        assert!(!is_newer("0.1.2.0", "0.1.2"));
        assert!(is_newer("0.2", "0.1.2"));
        assert!(is_newer("1", "0.1.2"));
        assert!(is_newer("0.1.2.1", "0.1.2"));
    }

    #[test]
    fn an_unreadable_version_on_either_side_is_never_newer() {
        for tag in [
            "",
            "   ",
            "v",
            "nightly",
            "1.2.3-rc1",
            "1.2.3+build",
            "1..2",
            "1.2.",
            ".1.2",
            "1.-2",
            "0x10",
            "٩.٩",
            "99999999999999999999999",
        ] {
            assert!(!is_newer(tag, "0.1.2"), "{tag:?} must not read as newer");
            assert!(!is_newer("9.9.9", tag), "{tag:?} must not be compared to");
        }
    }

    /// A version this parser cannot read would silence the check permanently,
    /// and silently. A host carries the same three assertions over its own
    /// `env!("CARGO_PKG_VERSION")`, which is the string this module actually
    /// compares against and the one only that crate can name.
    #[test]
    fn a_version_of_the_shape_the_applications_publish_is_readable() {
        for version in ["0.1.0", "0.2.0", "1.10.3", "2.0.0"] {
            assert!(
                parse_version(version).is_some(),
                "{version} is not a version `parse_version` understands"
            );
            assert!(is_newer("999.0.0", version));
            assert!(!is_newer(version, version));
        }
    }

    #[test]
    fn a_release_response_yields_its_tag_and_page() {
        fake();
        // Trimmed to the fields that matter; the real payload carries dozens
        // more, which is why the parser reaches for keys by name.
        let body = r#"{
            "tag_name": "v0.2.0",
            "name": "widget 0.2.0",
            "draft": false,
            "html_url": "https://github.com/xcomart/widget/releases/tag/v0.2.0",
            "assets": []
        }"#;
        let release = parse_release(body).expect("a well-formed release");
        assert_eq!(release.tag, "v0.2.0");
        assert_eq!(release.version, "0.2.0");
        assert_eq!(
            release.url,
            "https://github.com/xcomart/widget/releases/tag/v0.2.0"
        );
        assert_eq!(
            release_url(&release),
            "https://github.com/xcomart/widget/releases/tag/v0.2.0"
        );
        // No assets at all is the browser-fallback case on every platform.
        assert!(release.asset.is_none());
    }

    #[test]
    fn a_release_without_a_page_falls_back_to_the_releases_index() {
        fake();
        let release = parse_release(r#"{"tag_name":"0.2.0"}"#).expect("a tag is enough");
        assert_eq!(release.tag, "0.2.0");
        assert_eq!(release.version, "0.2.0");
        assert!(release.url.is_empty());
        assert!(release.asset.is_none());
        assert_eq!(release_url(&release), fake().releases_page);
    }

    #[test]
    fn a_response_without_a_usable_tag_is_no_release() {
        fake();
        for body in [
            "",
            "not json at all",
            "<html>captive portal</html>",
            "null",
            "[]",
            r#"{"message":"API rate limit exceeded"}"#,
            r#"{"tag_name":null}"#,
            r#"{"tag_name":42}"#,
            r#"{"tag_name":""}"#,
            r#"{"tag_name":"   "}"#,
        ] {
            assert!(parse_release(body).is_none(), "{body:?} must yield nothing");
        }
    }

    #[test]
    fn a_surrounding_whitespace_only_differs_by_trimming() {
        fake();
        let release = parse_release(r#"{"tag_name":"  v1.2.3  "}"#).expect("a padded tag");
        assert_eq!(release.tag, "v1.2.3");
        assert_eq!(release.version, "1.2.3");
    }

    #[test]
    fn an_asset_name_follows_the_release_workflow() {
        fake();
        assert_eq!(
            asset_name("v0.2.0", "x86_64-pc-windows-msvc"),
            "widget-v0.2.0-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(
            asset_name("v0.2.0", "aarch64-apple-darwin"),
            "widget-v0.2.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            asset_name("v0.2.0", "x86_64-unknown-linux-gnu"),
            "widget-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    /// A response carrying all three published assets, as the API shapes them.
    fn three_assets(tag: &str) -> String {
        let entries: Vec<String> = [
            "x86_64-pc-windows-msvc",
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
        ]
        .iter()
        .map(|target| {
            let name = asset_name(tag, target);
            format!(
                r#"{{"name":"{name}",
                    "size":1234,
                    "digest":"sha256:{hex}",
                    "browser_download_url":"https://example.invalid/{name}"}}"#,
                hex = "ab".repeat(32)
            )
        })
        .collect();
        format!(r#"{{"tag_name":"{tag}","assets":[{}]}}"#, entries.join(","))
    }

    #[test]
    fn the_asset_for_this_target_is_the_one_picked() {
        fake();
        let release = parse_release(&three_assets("v9.9.9")).expect("a well-formed release");
        match TARGET {
            // The three targets the project publishes: exactly one entry of the
            // response is the right one, and it is chosen by name.
            Some(target) => {
                let asset = release.asset.expect("a build for a published target");
                assert_eq!(asset.name, asset_name("v9.9.9", target));
                assert!(asset.url.ends_with(&asset.name));
                assert_eq!(asset.size, 1234);
                assert_eq!(asset.digest.as_deref(), Some("ab".repeat(32).as_str()));
            }
            // Everything else — an Intel Mac, an ARM Linux box — has no build
            // to install and must fall back to the browser.
            None => assert!(release.asset.is_none()),
        }
    }

    #[test]
    fn an_asset_for_another_tag_is_not_this_release() {
        fake();
        // The name carries the tag, so a response whose assets were built for a
        // different one matches nothing and degrades to the browser fallback.
        let body =
            three_assets("v9.9.9").replace("\"tag_name\":\"v9.9.9\"", "\"tag_name\":\"v8.8.8\"");
        let release = parse_release(&body).expect("a well-formed release");
        assert_eq!(release.tag, "v8.8.8");
        assert!(release.asset.is_none());
    }

    #[test]
    fn an_asset_without_a_download_url_is_no_asset() {
        fake();
        let Some(target) = TARGET else { return };
        let name = asset_name("v9.9.9", target);
        for entry in [
            format!(r#"{{"name":"{name}","size":1}}"#),
            format!(r#"{{"name":"{name}","browser_download_url":""}}"#),
            format!(r#"{{"name":"{name}","browser_download_url":42}}"#),
        ] {
            let body = format!(r#"{{"tag_name":"v9.9.9","assets":[{entry}]}}"#);
            let release = parse_release(&body).expect("a well-formed release");
            assert!(release.asset.is_none(), "{entry} must not be usable");
        }
    }

    #[test]
    fn an_asset_may_arrive_without_a_size_or_a_digest() {
        fake();
        let Some(target) = TARGET else { return };
        let name = asset_name("v9.9.9", target);
        let body = format!(
            r#"{{"tag_name":"v9.9.9","assets":[
                {{"name":"{name}","browser_download_url":"https://example.invalid/a"}}]}}"#
        );
        let asset = parse_release(&body)
            .and_then(|release| release.asset)
            .expect("a usable asset");
        // A zero size disables the byte-count check rather than failing it.
        assert_eq!(asset.size, 0);
        assert_eq!(asset.digest, None);
    }

    #[test]
    fn only_a_well_formed_sha256_digest_is_kept() {
        fake();
        let sha = "ab".repeat(32);
        assert_eq!(parse_digest(&format!("sha256:{sha}")), Some(sha.clone()));
        assert_eq!(
            parse_digest(&format!("SHA256:{}", sha.to_uppercase())),
            Some(sha)
        );
        for raw in [
            "",
            "sha256",
            "sha256:",
            "sha512:{}",
            &format!("sha512:{}", "ab".repeat(32)),
            &format!("sha256:{}", "ab".repeat(31)),
            &format!("sha256:{}", "zz".repeat(32)),
        ] {
            assert_eq!(parse_digest(raw), None, "{raw:?} must not be accepted");
        }
    }

    #[test]
    fn a_digest_is_compared_as_lower_case_hex() {
        fake();
        // The empty input's SHA-256, so the encoder is checked against a value
        // that is not of this codebase's making.
        let digest = ring::digest::digest(&ring::digest::SHA256, b"");
        assert_eq!(
            hex(digest.as_ref()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn the_displaced_copy_keeps_its_whole_name() {
        for (path, expected) in [
            ("C:/Program Files/widget/widget.exe", "widget.exe.old"),
            ("/usr/local/share/widget/widget", "widget.old"),
            ("/usr/local/share/widget/runtime", "runtime.old"),
            ("/Applications/widget.app", "widget.app.old"),
        ] {
            let retired = old_path(Path::new(path)).expect("a path with a file name");
            assert_eq!(
                retired.file_name().and_then(|name| name.to_str()),
                Some(expected)
            );
            assert_eq!(retired.parent(), Path::new(path).parent());
        }
        assert_eq!(old_path(Path::new("/")), None);
    }

    #[test]
    fn a_bundle_is_found_however_deep_the_binary_sits() {
        assert_eq!(
            bundle_root(Path::new("/Applications/widget.app/Contents/MacOS/widget")),
            Some(PathBuf::from("/Applications/widget.app"))
        );
        // The extension is what identifies it, not the depth or the name.
        assert_eq!(
            bundle_root(Path::new("/tmp/x/Some Name.APP/Contents/MacOS/widget")),
            Some(PathBuf::from("/tmp/x/Some Name.APP"))
        );
        // A development build, and a binary copied out of its bundle: nothing
        // to swap, which is what makes the macOS install refuse.
        assert_eq!(
            bundle_root(Path::new("/work/widget/target/debug/widget")),
            None
        );
        assert_eq!(bundle_root(Path::new("/usr/local/bin/widget")), None);
    }

    #[test]
    fn an_archive_name_can_never_leave_the_staging_directory() {
        let app = fake();
        assert_eq!(
            archive_name("widget-v0.2.0-x86_64-pc-windows-msvc.zip"),
            "widget-v0.2.0-x86_64-pc-windows-msvc.zip"
        );
        for hostile in ["", ".", "..", "../evil", "a/b", "a\\b", "/etc/passwd"] {
            assert_eq!(archive_name(hostile), app.fallback_archive, "{hostile:?}");
        }
    }

    /// The three entries a non-macOS archive of a multi-file application
    /// carries, as names.
    ///
    /// Also [`FAKE`]'s payload on every platform, macOS included, so that the
    /// tests below exercise the multi-entry shape everywhere: the swap logic is
    /// the same code there, and a one-entry payload would leave the rollback
    /// untested on exactly the platform that cannot run it. A single-file
    /// application is the one-entry case of this, which
    /// [`a_single_entry_payload_swaps_and_rolls_back_like_any_other`] covers.
    const TRIPLE: [&str; 3] = ["widget", "lib", "runtime"];

    /// Builds a directory holding `names`, the file entries carrying `mark`.
    ///
    /// `lib` and `runtime` are made as directories with a file inside, because
    /// that is what they are on disk and because a rename of a directory is the
    /// operation the swap actually has to perform.
    fn tree(root: &Path, names: &[&str], mark: &str) {
        fs::create_dir_all(root).expect("a directory");
        for name in names {
            if *name == "lib" || *name == "runtime" {
                let dir = root.join(name);
                fs::create_dir_all(&dir).expect("a directory");
                fs::write(dir.join("payload.txt"), mark).expect("a file");
            } else {
                fs::write(root.join(name), mark).expect("a file");
            }
        }
    }

    /// What a file (or a directory's `payload.txt`) says.
    fn mark_of(root: &Path, name: &str) -> String {
        let path = root.join(name);
        let path = if path.is_dir() {
            path.join("payload.txt")
        } else {
            path
        };
        fs::read_to_string(path).expect("a readable entry")
    }

    /// A plan replacing every one of [`TRIPLE`] inside `install`.
    fn plan_over(install: &Path) -> Vec<Entry> {
        TRIPLE
            .iter()
            .map(|name| Entry {
                target: install.join(name),
                source: name,
            })
            .collect()
    }

    #[test]
    fn the_payload_is_found_at_the_root_or_one_level_down() {
        let root = tempfile::tempdir().expect("a temp directory");
        let root = root.path();

        // Nothing there yet.
        assert_eq!(find_payload(root, &TRIPLE), None);

        // The shape every published archive has: one wrapper directory.
        let wrapper = root.join("widget-v0.2.0-x86_64-unknown-linux-gnu");
        tree(&wrapper, &TRIPLE, "new");
        assert_eq!(find_payload(root, &TRIPLE), Some(wrapper.clone()));

        // A flat archive works too, and wins, because it is unambiguous.
        tree(root, &TRIPLE, "new");
        assert_eq!(find_payload(root, &TRIPLE), Some(root.to_path_buf()));

        // A directory counts as a payload: that is what the macOS bundle is,
        // and what `lib` and `runtime` are everywhere else.
        let bundles = tempfile::tempdir().expect("a temp directory");
        fs::create_dir_all(bundles.path().join("wrapper/widget.app/Contents")).expect("a bundle");
        assert_eq!(
            find_payload(bundles.path(), &["widget.app"]),
            Some(bundles.path().join("wrapper"))
        );
    }

    /// A wrapper that carries only part of the payload is not the payload.
    ///
    /// The case this guards is a Windows archive whose `runtime/` failed to
    /// pack: matching on the executable alone would move the new binary in and
    /// then fail on the directory, which is exactly the half-swapped state the
    /// rollback exists to prevent — better never to start.
    #[test]
    fn a_directory_missing_one_entry_is_not_the_payload() {
        let root = tempfile::tempdir().expect("a temp directory");
        let wrapper = root.path().join("widget-v0.2.0-x86_64-pc-windows-msvc");
        tree(&wrapper, &["widget", "lib"], "new");
        assert_eq!(find_payload(root.path(), &TRIPLE), None);
        // And with the last one in place it is.
        tree(&wrapper, &TRIPLE, "new");
        assert_eq!(find_payload(root.path(), &TRIPLE), Some(wrapper));
    }

    #[test]
    fn a_swap_replaces_every_entry_and_keeps_the_old_ones_aside() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let payload = root.path().join("payload");
        tree(&install, &TRIPLE, "old");
        tree(&payload, &TRIPLE, "new");

        swap(&plan_over(&install), &payload).expect("every entry moves");

        for name in TRIPLE {
            assert_eq!(mark_of(&install, name), "new", "{name} was not replaced");
            let retired = format!("{name}{OLD_SUFFIX}");
            assert!(
                install.join(&retired).exists(),
                "{retired} should have been kept aside"
            );
            assert_eq!(mark_of(&install, &retired), "old");
        }
    }

    /// The whole reason the swap keeps a journal: one entry that cannot move
    /// must leave the installation exactly as it was.
    #[test]
    fn a_failed_entry_rolls_the_earlier_ones_back() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let payload = root.path().join("payload");
        tree(&install, &TRIPLE, "old");
        // The payload is missing `runtime`, so the third rename fails after the
        // first two have already happened.
        tree(&payload, &["widget", "lib"], "new");

        let error = swap(&plan_over(&install), &payload).expect_err("the third entry cannot move");
        assert!(error.contains("runtime"), "{error}");
        // No mention of an incomplete rollback: this one had to succeed.
        assert!(!error.contains("rollback"), "{error}");

        for name in TRIPLE {
            assert_eq!(
                mark_of(&install, name),
                "old",
                "{name} should have been rolled back"
            );
            assert!(
                !install.join(format!("{name}{OLD_SUFFIX}")).exists(),
                "{name} should have no leftover after a rollback"
            );
        }
    }

    /// An entry with nothing installed under its name still installs, and a
    /// rollback removes it again rather than leaving half a tree.
    #[test]
    fn an_entry_with_nothing_to_displace_is_still_undone() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let payload = root.path().join("payload");
        // A development tree: the binary is there, the two directories beside
        // it are not.
        tree(&install, &["widget"], "old");
        tree(&payload, &["widget", "lib"], "new");

        swap(&plan_over(&install), &payload).expect_err("`runtime` is missing from the payload");

        assert_eq!(mark_of(&install, "widget"), "old");
        assert!(
            !install.join("lib").exists(),
            "the freshly created lib/ should have been taken away again"
        );
        assert!(!install.join("widget.old").exists());
    }

    /// The staging half: the payload leaves the scratch tree in one rename.
    #[test]
    fn a_deferred_update_is_parked_beside_the_installation() {
        let root = tempfile::tempdir().expect("a temp directory");
        let staging = root.path().join(".update");
        let payload = staging.join("unpacked/widget-v0.2.0-x86_64-pc-windows-msvc");
        let pending = root.path().join(PENDING_DIR);
        tree(&payload, &TRIPLE, "new");

        defer(&payload, &pending).expect("the payload moves out of the staging tree");

        assert!(!payload.exists(), "the payload should have been moved");
        for name in TRIPLE {
            assert_eq!(mark_of(&pending, name), "new");
        }

        // The staging directory goes at the end of every install, and the
        // parked payload has to outlive it.
        remove(&staging).expect("the staging tree is removable");
        assert!(holds_all(&pending, &TRIPLE));

        // A payload staged and never applied is replaced, not merged into.
        let second = staging.join("unpacked");
        tree(&second, &["widget"], "newer");
        defer(&second, &pending).expect("the second payload takes the first one's place");
        assert_eq!(mark_of(&pending, "widget"), "newer");
        assert!(
            !pending.join("lib").exists(),
            "the superseded payload should be gone entirely"
        );
    }

    #[test]
    fn a_complete_staged_update_is_applied_and_then_cleared() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let pending = install.join(PENDING_DIR);
        tree(&install, &TRIPLE, "old");
        tree(&pending, &TRIPLE, "new");

        assert!(apply(&plan_over(&install), &pending));

        for name in TRIPLE {
            assert_eq!(mark_of(&install, name), "new", "{name} was not replaced");
            assert_eq!(mark_of(&install, &format!("{name}{OLD_SUFFIX}")), "old");
        }
        assert!(
            !pending.exists(),
            "the pending directory must not survive its own application"
        );
    }

    /// An archive that unpacked badly, or a payload someone pruned: the swap
    /// must not start, and the directory must not be left to fail again on
    /// every launch from here on.
    #[test]
    fn an_incomplete_staged_update_is_discarded_without_touching_anything() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let pending = install.join(PENDING_DIR);
        tree(&install, &TRIPLE, "old");
        tree(&pending, &["widget", "lib"], "new");

        assert!(!apply(&plan_over(&install), &pending));

        for name in TRIPLE {
            assert_eq!(mark_of(&install, name), "old", "{name} should be untouched");
            assert!(!install.join(format!("{name}{OLD_SUFFIX}")).exists());
        }
        assert!(!pending.exists(), "the pending directory should be gone");
    }

    #[test]
    fn a_staged_update_that_cannot_be_swapped_rolls_back_and_is_still_cleared() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let pending = install.join(PENDING_DIR);
        tree(&install, &TRIPLE, "old");
        // A fourth entry whose target sits in a directory that does not exist,
        // so the pending directory passes the completeness check and the rename
        // fails anyway — after the first three have already happened.
        tree(&pending, &["widget", "lib", "runtime", "extra"], "new");
        let mut plan = plan_over(&install);
        plan.push(Entry {
            target: install.join("nowhere").join("extra"),
            source: "extra",
        });

        assert!(!apply(&plan, &pending));

        for name in TRIPLE {
            assert_eq!(
                mark_of(&install, name),
                "old",
                "{name} should have been rolled back"
            );
            assert!(!install.join(format!("{name}{OLD_SUFFIX}")).exists());
        }
        assert!(
            !pending.exists(),
            "a failed application must clear the directory too, or it fails forever"
        );
    }

    #[test]
    fn the_relaunch_target_is_something_that_can_be_executed() {
        let plan = vec![Entry {
            target: PathBuf::from(if cfg!(target_os = "macos") {
                "/Applications/widget.app"
            } else {
                "/opt/widget/widget"
            }),
            source: fake().payload[0],
        }];
        let exe = relaunch_target(&plan).expect("a plan with a first entry");
        #[cfg(target_os = "macos")]
        assert_eq!(
            exe,
            PathBuf::from("/Applications/widget.app/Contents/MacOS/widget")
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(exe, PathBuf::from("/opt/widget/widget"));
        assert_eq!(relaunch_target(&[]), None);
    }

    #[test]
    fn a_release_with_no_asset_cannot_be_installed() {
        fake();
        // The one `install` failure reachable without touching the network or
        // the filesystem, and the one that must never be a panic: it is what
        // an unpublished target reaches if the dialog ever routes it here.
        let release = Release {
            tag: "v9.9.9".to_string(),
            version: "9.9.9".to_string(),
            url: String::new(),
            asset: None,
        };
        let mut seen = Vec::new();
        let error = install(&release, &mut |progress| seen.push(progress))
            .expect_err("no asset, no install");
        assert!(error.contains("v9.9.9"), "{error}");
        assert!(seen.is_empty(), "nothing should have been reported");
    }

    #[test]
    fn a_directory_is_the_same_as_itself_however_it_is_written() {
        let installed = tempfile::tempdir().expect("a temporary directory");
        let path = installed.path();
        assert!(same_directory(path, path));
        // The shape Inno Setup actually stores. Comparing the two as text would
        // fail here, which is the whole reason the comparison canonicalises.
        let trailing = format!("{}{}", path.display(), std::path::MAIN_SEPARATOR);
        assert!(same_directory(Path::new(&trailing), path));
        // And a leading or trailing space of the kind a hand-edited registry
        // value collects, which the caller trims before asking.
        assert!(same_directory(
            Path::new(format!("  {trailing} ").trim()),
            path
        ));
    }

    #[test]
    fn a_different_missing_or_merely_nested_directory_is_not_the_same() {
        let installed = tempfile::tempdir().expect("a temporary directory");
        let elsewhere = tempfile::tempdir().expect("a second temporary directory");
        assert!(!same_directory(installed.path(), elsewhere.path()));

        // A parent or a child is a near miss and still a miss: a portable copy
        // unpacked inside the installed copy's directory is not that install.
        let nested = installed.path().join("runtime");
        fs::create_dir(&nested).expect("a subdirectory");
        assert!(!same_directory(&nested, installed.path()));
        assert!(!same_directory(installed.path(), &nested));

        // Nothing there to canonicalise. The answer is "no", never a panic:
        // an entry pointing at a directory that is gone is someone else's
        // broken installation.
        assert!(!same_directory(
            &installed.path().join("gone"),
            installed.path()
        ));
    }

    /// A registry key under `HKCU\Software` that removes itself when the test
    /// holding it ends.
    ///
    /// The real uninstall key is a live part of this machine's installed-program
    /// list, and the tests below write to a scratch key instead — which is what
    /// [`write_display_version`] takes its key path as an argument for. The name
    /// carries a UUID so that two tests running on the same machine, in the same
    /// process or not, cannot collide.
    /// A suffix no other run of this suite shares.
    ///
    /// The process id and a counter rather than a random identifier: a random
    /// one would mean a dependency on a generator for a name whose only job is
    /// to be unique among concurrently running tests, and these two already
    /// are.
    #[cfg(windows)]
    fn scratch_suffix() -> String {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[cfg(windows)]
    struct ScratchKey(String);

    #[cfg(windows)]
    impl ScratchKey {
        fn new() -> Self {
            let path = format!("Software\\ruui-shell-test-{}", scratch_suffix());
            windows_registry::CURRENT_USER
                .create(&path)
                .expect("a scratch registry key under HKCU");
            Self(path)
        }

        /// The key itself, opened for reading and writing.
        fn open(&self) -> windows_registry::Key {
            windows_registry::CURRENT_USER
                .options()
                .read()
                .write()
                .open(&self.0)
                .expect("the scratch key this test just created")
        }
    }

    #[cfg(windows)]
    impl Drop for ScratchKey {
        fn drop(&mut self) {
            let _ = windows_registry::CURRENT_USER.remove_tree(&self.0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn an_entry_describing_this_copy_has_its_version_rewritten() {
        let installed = tempfile::tempdir().expect("a temporary directory");
        let scratch = ScratchKey::new();
        let key = scratch.open();
        // Written with the trailing backslash Inno leaves, so the comparison is
        // exercised against the real shape and not a tidied one.
        key.set_string(
            INSTALL_LOCATION,
            format!("{}\\", installed.path().display()),
        )
        .expect("an install location");
        key.set_string(DISPLAY_VERSION, "0.1.6")
            .expect("a starting version");

        assert!(write_display_version(
            windows_registry::CURRENT_USER,
            "HKCU",
            &scratch.0,
            installed.path(),
            "0.1.7",
        ));
        assert_eq!(key.get_string(DISPLAY_VERSION).expect("a version"), "0.1.7");
    }

    #[cfg(windows)]
    #[test]
    fn an_entry_describing_another_copy_is_left_alone() {
        // The case the guard exists for: an installed copy and a portable one on
        // the same machine, and the portable one updating itself. Marking the
        // installed copy up to date would take it out of `winget upgrade`
        // forever while its files stayed where they were.
        let installed = tempfile::tempdir().expect("a temporary directory");
        let portable = tempfile::tempdir().expect("a second temporary directory");
        let scratch = ScratchKey::new();
        let key = scratch.open();
        key.set_string(INSTALL_LOCATION, installed.path().display().to_string())
            .expect("an install location");
        key.set_string(DISPLAY_VERSION, "0.1.6")
            .expect("a starting version");

        assert!(!write_display_version(
            windows_registry::CURRENT_USER,
            "HKCU",
            &scratch.0,
            portable.path(),
            "0.1.7",
        ));
        assert_eq!(
            key.get_string(DISPLAY_VERSION).expect("a version"),
            "0.1.6",
            "the other installation's recorded version must not move"
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_entry_that_describes_nothing_is_left_alone() {
        // An uninstall key with no `InstallLocation` cannot be matched against
        // anything, so it is not this copy's to edit either.
        let installed = tempfile::tempdir().expect("a temporary directory");
        let scratch = ScratchKey::new();
        let key = scratch.open();
        key.set_string(DISPLAY_VERSION, "0.1.6")
            .expect("a starting version");

        assert!(!write_display_version(
            windows_registry::CURRENT_USER,
            "HKCU",
            &scratch.0,
            installed.path(),
            "0.1.7",
        ));
        assert_eq!(key.get_string(DISPLAY_VERSION).expect("a version"), "0.1.6");
    }

    #[cfg(windows)]
    #[test]
    fn an_entry_that_is_not_there_is_not_created() {
        // What a copy unpacked from the zip looks like, and the one outcome that
        // would be actively harmful: an "Apps & features" entry whose uninstall
        // command points at an uninstaller that was never installed.
        let installed = tempfile::tempdir().expect("a temporary directory");
        let absent = format!("Software\\ruui-shell-test-{}", scratch_suffix());

        assert!(!write_display_version(
            windows_registry::CURRENT_USER,
            "HKCU",
            &absent,
            installed.path(),
            "0.1.7",
        ));
        assert!(
            windows_registry::CURRENT_USER.open(&absent).is_err(),
            "the updater must never bring an uninstall entry into existence"
        );
    }

    /// A single-file application is the one-entry case of everything above.
    ///
    /// One of the three applications sharing this shell publishes exactly one
    /// file per release, and the multi-entry machinery has to reduce to what it
    /// used to do by hand: one rename aside, one rename in, and — when the new
    /// copy is not there — the old one straight back with nothing left behind.
    #[test]
    fn a_single_entry_payload_swaps_and_rolls_back_like_any_other() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let payload = root.path().join("payload");
        tree(&install, &["widget"], "old");
        tree(&payload, &["widget"], "new");
        let plan = vec![Entry {
            target: install.join("widget"),
            source: "widget",
        }];

        swap(&plan, &payload).expect("the one entry moves");
        assert_eq!(mark_of(&install, "widget"), "new");
        assert_eq!(mark_of(&install, &format!("widget{OLD_SUFFIX}")), "old");

        // And with nothing to move in, the displaced copy comes back.
        let empty = root.path().join("empty");
        fs::create_dir_all(&empty).expect("a directory");
        remove(&install.join(format!("widget{OLD_SUFFIX}"))).expect("a removable leftover");
        let error = swap(&plan, &empty).expect_err("there is nothing to install");
        assert!(error.contains("widget"), "{error}");
        assert_eq!(mark_of(&install, "widget"), "new");
        assert!(!install.join(format!("widget{OLD_SUFFIX}")).exists());
    }

    /// The staged path, over a one-entry payload: a single-file application
    /// that has to defer still applies its update on the next launch.
    #[test]
    fn a_single_entry_payload_stages_and_applies() {
        let root = tempfile::tempdir().expect("a temp directory");
        let install = root.path().join("install");
        let pending = install.join(PENDING_DIR);
        tree(&install, &["widget"], "old");
        tree(&pending, &["widget"], "new");
        let plan = vec![Entry {
            target: install.join("widget"),
            source: "widget",
        }];

        assert!(apply(&plan, &pending));
        assert_eq!(mark_of(&install, "widget"), "new");
        assert!(!pending.exists());
    }

    /// The start-up check is a request, and a suite that builds windows must
    /// not make dozens of them.
    #[test]
    fn the_start_up_check_can_be_turned_off_without_touching_the_manual_one() {
        fake();
        set_startup_check_enabled(false);
        // No request is made, so this returns without waiting on the network.
        assert_eq!(check(None), None);
        assert_eq!(check(Some("v9.9.9")), None);
        set_startup_check_enabled(true);
        assert!(STARTUP_CHECK.load(Ordering::Relaxed));
        // Put it back the way a suite that shares this process wants it.
        set_startup_check_enabled(false);
    }
}
