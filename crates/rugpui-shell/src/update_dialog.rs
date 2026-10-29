//! The update dialog: announcing a release, installing it, and reporting how
//! that went.
//!
//! Reached two ways, and the difference between them is the reason this is a
//! state machine rather than a message box.
//!
//! * The **start-up check** opens it already knowing there is something to say,
//!   so it appears in [`State::Announce`] with three ways out — Update, Ignore
//!   this version, Cancel. "Not now" and "not ever" are different answers, and
//!   collapsing them would either nag a user who has decided to stay put or
//!   silence one who merely had no time today.
//! * **Check for updates** opens it *before* there is an answer, in
//!   [`State::Checking`], and it becomes an announcement, an "up to date", or a
//!   failure depending on what GitHub says. That path deliberately ignores the
//!   remembered "ignore this version" tag: the user has just overruled it by
//!   asking. Ignoring it again is still on offer, from the same button as ever.
//!
//! An announcement also carries the release's own notes — its summary and
//! bullets, read by [`crate::release_notes`] — in a scrolling box under the two
//! version lines, so the user can judge the update before taking it.
//!
//! Pressing Update moves into [`State::Busy`], where the dialog owns a download
//! and a swap running on the background executor and draws a progress bar.
//! There are no buttons in that state and `Escape` does nothing — see
//! [`UpdateDialog::close`] for why interrupting is not offered — and it ends
//! either in [`UpdateDialogEvent::Installed`] or in [`State::Failed`], which
//! offers the browser hand-off.
//!
//! # What belongs where
//!
//! The dialog owns its own state and its own background work. It owns neither
//! end of what happens around them:
//!
//! * **The restart is the host's.** A successful install ends in
//!   [`UpdateDialogEvent::Installed`], and what the host does with it is
//!   `cx.restart()` — possibly after `cx.set_restart_path(…)`, which is what an
//!   application whose executable moves during the swap needs and which only it
//!   can know. A dialog that called `cx.restart()` itself would take that
//!   choice away.
//! * **The settings are the host's**, so "ignore this version" is an event too:
//!   the tag travels out and the host writes the file.
//!
//! Structurally a twin of [`crate::AboutDialog`] — same `open`/`close`/
//! `is_open` shape, same `Escape` handling, same "renders nothing while closed"
//! contract — so a host wires it up the same way.

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, App, Context, DragMoveEvent, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, IntoElement, KeyDownEvent, MouseButton, MouseUpEvent, Render, ScrollHandle,
    SharedString, StyledText, Window, div, prelude::*, px, relative,
};
use rugpui::{
    Button, ButtonVariant, DraggedThumb, Scrollbar, ScrollbarAxis, ScrollbarState, SmoothScrollExt,
    Theme, hide_later, hide_now, modal, scroll_to, scrolled, theme,
};

use crate::inject::{identity, label, text};
use crate::release_notes::{Block, Inline};
use crate::settings::monospace_family;
use crate::update::{self, Check, Installed, Progress, Release, release_url};

/// Width of the dialog panel. Matches the about dialog: both are a short
/// paragraph and a row of buttons.
const DIALOG_WIDTH: f32 = 420.;

/// Width of the panel while it shows release notes: a bullet of them is a
/// sentence or two, which wraps into a column at the narrower width.
const NOTES_WIDTH: f32 = 540.;

/// Height at which the release notes start scrolling, so a long release body
/// never pushes the buttons off a small window.
const NOTES_MAX_HEIGHT: f32 = 280.;

/// Element id of the release notes' overlay scroll indicator.
const NOTES_SCROLLBAR_ID: &str = "update-notes-scrollbar";

/// Height of the download progress bar.
const PROGRESS_BAR: f32 = 4.;

/// Emitted by [`UpdateDialog`].
pub enum UpdateDialogEvent {
    /// The user asked never to be told about this release again. The host
    /// persists the tag; the dialog has already closed itself.
    Ignored {
        /// The release tag, exactly as GitHub published it.
        tag: String,
    },
    /// The new build is on disk and the host should restart into it.
    ///
    /// Both variants of [`Installed`] arrive here and both mean the same thing
    /// to a host: restart. A staged one merely spends its restart applying
    /// itself and then re-executing once more, which the user cannot tell apart
    /// from the direct case.
    ///
    /// The dialog is still on screen when this arrives, showing the last state
    /// of the progress bar, because the restart is imminent and a dialog that
    /// closed itself first would flash the window back into view for the
    /// fraction of a second before the process goes.
    Installed(Installed),
    /// The dialog was dismissed — by Cancel, by Close, by `Escape`, by the
    /// backdrop, or by the browser having been sent to the release page. The
    /// host should restore focus.
    Dismissed,
}

/// What the dialog is doing, and with it whether it is on screen at all.
///
/// One enum rather than a `bool` beside a pile of options: every combination
/// this names is drawable, and no combination it cannot name is.
enum State {
    /// Not on screen.
    Closed,
    /// A check the user asked for is in flight.
    Checking,
    /// A newer release exists and the user has not answered yet.
    Announce(Release),
    /// The release is being downloaded and installed. No way out but the end.
    Busy {
        /// What is being installed, kept so a failure can still offer its page.
        release: Release,
        /// How far along it is.
        phase: Phase,
    },
    /// A check the user asked for found nothing newer.
    UpToDate,
    /// A check or an install did not complete.
    Failed {
        /// The release involved, when there was one. `None` for a check that
        /// never got far enough to name a release, which is what decides
        /// whether the "open the release page" button appears.
        release: Option<Release>,
        /// Untranslated technical detail, shown under the translated heading.
        /// See [`crate::update::install`] for why it is not translated.
        message: String,
    },
}

/// The two halves of an install, as the progress line reports them.
enum Phase {
    /// Bytes are arriving.
    Downloading {
        /// Bytes received so far.
        done: u64,
        /// Bytes expected in total; zero when the release did not say.
        total: u64,
    },
    /// The archive is being unpacked and moved into place.
    Installing,
}

/// Modal dialog for everything to do with updating.
///
/// Create it once with [`UpdateDialog::new`], keep the handle, subscribe to
/// [`UpdateDialogEvent`], and render it as the last child of a `relative()`
/// root. It renders nothing while [`UpdateDialog::is_open`] is `false`, so it is
/// safe to render unconditionally.
pub struct UpdateDialog {
    /// What the dialog is showing.
    state: State,
    /// Focus of the dialog root; also the anchor for the `Escape` handler.
    focus_handle: FocusHandle,
    /// Whether focus should move into the dialog on the next render.
    pending_focus: bool,
    /// Scroll position of the release notes.
    notes_scroll: ScrollHandle,
    /// Whether the release notes' overlay scroll indicator is on screen.
    notes_scrollbar: ScrollbarState,
}

impl UpdateDialog {
    /// Builds the dialog, closed.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            state: State::Closed,
            focus_handle: cx.focus_handle(),
            pending_focus: false,
            notes_scroll: ScrollHandle::new(),
            notes_scrollbar: ScrollbarState::new(),
        }
    }

    /// Puts `release` up, with its notes scrolled back to the top.
    fn announce(&mut self, release: Release) {
        self.notes_scroll = ScrollHandle::new();
        self.notes_scrollbar = ScrollbarState::new();
        self.state = State::Announce(release);
    }

    /// Shows the dialog, announcing `release`.
    ///
    /// The start-up check's entry point: it has already decided there is
    /// something worth saying.
    pub fn open(&mut self, release: Release, cx: &mut Context<Self>) {
        self.announce(release);
        self.pending_focus = true;
        cx.notify();
    }

    /// Shows the dialog and asks GitHub, from the menu item.
    ///
    /// Opens *before* the answer so the click has a visible effect on a slow
    /// connection, and so the user can walk away from the question by
    /// dismissing it. A check the user canceled that way stays canceled: the
    /// answer is applied only if the dialog is still in [`State::Checking`] when
    /// it lands.
    ///
    /// Deliberately does not consult the ignored-version tag. Asking is an
    /// override of the earlier "don't mention this again", and answering "up to
    /// date" to a user looking at an ignored release would be a lie.
    pub fn start_check(&mut self, cx: &mut Context<Self>) {
        self.state = State::Checking;
        self.pending_focus = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async { update::check_now() })
                .await;
            this.update(cx, |dialog, cx| dialog.report_check(outcome, cx))
                .ok();
        })
        .detach();
    }

    /// Whether the dialog is visible.
    pub fn is_open(&self) -> bool {
        !matches!(self.state, State::Closed)
    }

    /// Whether an install is running, and the dialog therefore refuses to go
    /// away.
    pub fn is_busy(&self) -> bool {
        matches!(self.state, State::Busy { .. })
    }

    /// Hides the dialog without emitting an event.
    ///
    /// A no-op while an install is running. The swap renames each installed
    /// entry aside before moving the new one in, so between those renames the
    /// application directory is a mixture of two builds; a dialog that could be
    /// dismissed mid-way would invite the user to close the window — and with
    /// it the process — exactly then. There is nothing to gain from allowing it
    /// either: closing the dialog would not stop the background task, only hide
    /// it.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        self.state = State::Closed;
        self.pending_focus = false;
        cx.notify();
    }

    /// Closes the dialog and reports it, so the shell can restore focus.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        cx.emit(UpdateDialogEvent::Dismissed);
        self.close(cx);
    }

    /// Applies the answer to a check the user asked for.
    ///
    /// Ignored unless the dialog is still waiting for it: by the time GitHub
    /// answers, the user may have dismissed the dialog, or the start-up check
    /// may have put an announcement up in the meantime.
    fn report_check(&mut self, outcome: Check, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Checking) {
            return;
        }
        match outcome {
            Check::Newer(release) => self.announce(release),
            Check::UpToDate => self.state = State::UpToDate,
            Check::Failed(message) => {
                self.state = State::Failed {
                    release: None,
                    message,
                };
            }
        }
        cx.notify();
    }

    /// Starts downloading and installing `release`.
    ///
    /// The work is blocking — an HTTPS transfer, a `tar`, a handful of renames —
    /// so it runs on the background executor and reports itself over a channel
    /// this task drains. The two are polled *together*, for the reason the
    /// extraction dialog's job loop is: awaiting the install and reading the
    /// counts afterwards would show a frozen bar for the whole download.
    ///
    /// Success ends in [`UpdateDialogEvent::Installed`], and the host restarts.
    /// `cx.restart()` spawns a watcher that waits for this pid to exit and
    /// starts the application again — from the same path, which by then holds
    /// the build that was just installed, either because the swap has already
    /// happened or because the next launch applies it before it does anything
    /// else. See [`crate::update::Installed`].
    fn install(&mut self, release: Release, cx: &mut Context<Self>) {
        let total = release.asset.as_ref().map_or(0, |asset| asset.size);
        self.state = State::Busy {
            release: release.clone(),
            phase: Phase::Downloading { done: 0, total },
        };
        cx.notify();

        cx.spawn(async move |this, cx| {
            let (sender, mut progress) = mpsc::unbounded();
            let job = release;
            let install = cx.background_executor().spawn(async move {
                update::install(&job, &mut |step| {
                    // The receiver is dropped only when this task is, which
                    // happens only when the dialog is; a send that fails means
                    // nobody is drawing the bar any more, and the install
                    // still has to finish.
                    let _ = sender.unbounded_send(step);
                })
            });

            let install = futures::FutureExt::fuse(install);
            futures::pin_mut!(install);

            let outcome = loop {
                futures::select! {
                    outcome = install => break outcome,
                    step = progress.next() => {
                        let Some(step) = step else { continue };
                        if this
                            .update(cx, |dialog, cx| dialog.advance(step, cx))
                            .is_err()
                        {
                            // The dialog is gone, but the swap is not: abandoning
                            // it here would leave the installation half-renamed
                            // with nobody to roll it back.
                            break install.await;
                        }
                    }
                }
            };

            match outcome {
                Ok(installed) => {
                    // Both outcomes are a success and both end in a restart the
                    // host performs. A staged one merely spends its restart
                    // applying itself and then re-executing once more, which the
                    // user cannot tell apart from the direct case — so only the
                    // log distinguishes them here.
                    if installed == Installed::Staged {
                        log::debug!("the update will be applied as this process exits");
                    }
                    this.update(cx, |_dialog, cx| {
                        cx.emit(UpdateDialogEvent::Installed(installed));
                    })
                    .ok();
                }
                Err(message) => {
                    this.update(cx, |dialog, cx| dialog.fail(message, cx)).ok();
                }
            }
        })
        .detach();
    }

    /// Records how far the running install has got.
    ///
    /// Repaints only when the whole percent moves: the sender already throttles
    /// to one report per quarter-megabyte, and on a fast connection that is
    /// still dozens of frames a second for a bar a pixel wide.
    fn advance(&mut self, step: Progress, cx: &mut Context<Self>) {
        let State::Busy { phase, .. } = &mut self.state else {
            return;
        };
        let visible = match (&*phase, step) {
            (
                Phase::Downloading {
                    done: shown,
                    total: known,
                },
                Progress::Downloading { done, total },
            ) => percent(done, total) != percent(*shown, *known),
            // Any other transition is a change of wording, which always shows.
            _ => true,
        };
        *phase = match step {
            Progress::Downloading { done, total } => Phase::Downloading { done, total },
            Progress::Installing => Phase::Installing,
        };
        if visible {
            cx.notify();
        }
    }

    /// Moves a running install into its error state.
    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        let release = match std::mem::replace(&mut self.state, State::Closed) {
            State::Busy { release, .. } | State::Announce(release) => Some(release),
            _ => None,
        };
        self.state = State::Failed { release, message };
        cx.notify();
    }

    /// What the primary button does for a release with no build to install.
    ///
    /// The original behavior, kept for the platforms the project publishes no
    /// asset for and for a release whose assets do not include the expected
    /// name: send the browser to the page and get out of the way.
    fn open_page(&mut self, release: &Release, cx: &mut Context<Self>) {
        cx.open_url(release_url(release));
        self.dismiss(cx);
    }

    /// Reports the release as unwanted, then closes.
    ///
    /// The event carries the tag because the dialog is about to forget it, and
    /// the shell — which owns the settings — needs it to write the file.
    fn ignore(&mut self, release: &Release, cx: &mut Context<Self>) {
        cx.emit(UpdateDialogEvent::Ignored {
            tag: release.tag.clone(),
        });
        self.close(cx);
    }

    /// `Escape` dismisses the dialog from anywhere inside it.
    ///
    /// Equivalent to Cancel, never to Ignore: a key pressed to make a surprise
    /// go away must not commit the user to anything. Inert during an install,
    /// for the reason on [`UpdateDialog::close`].
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_open() && event.keystroke.key == "escape" {
            cx.stop_propagation();
            self.dismiss(cx);
        }
    }

    /// Moves focus into the dialog when it opens, so `Escape` reaches it.
    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_focus {
            return;
        }
        self.pending_focus = false;
        let handle = self.focus_handle.clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    /// The release notes on screen, when the dialog is showing any.
    fn notes(&self) -> Option<&[Block]> {
        match &self.state {
            State::Announce(release) if !release.notes.is_empty() => Some(&release.notes),
            _ => None,
        }
    }

    /// The overlay scroll indicator of the release notes, as it stands.
    fn notes_scrollbar(&self) -> Scrollbar {
        Scrollbar::for_handle(
            NOTES_SCROLLBAR_ID,
            ScrollbarAxis::Vertical,
            &self.notes_scroll,
        )
        .fade(self.notes_scrollbar.fade())
    }

    /// Puts the bar up whenever the notes have been scrolled, and starts the
    /// clock that takes it down again.
    fn watch_scroll(&mut self, cx: &mut Context<Self>) {
        let scrolled = scrolled(&self.notes_scroll, ScrollbarAxis::Vertical);
        if let Some(epoch) = self.notes_scrollbar.moved(scrolled) {
            hide_later(epoch, cx, |dialog| Some(&mut dialog.notes_scrollbar));
        }
    }

    /// Scrolls the notes while their thumb is dragged.
    fn drag_scrollbar(&mut self, event: &DragMoveEvent<DraggedThumb>, cx: &mut Context<Self>) {
        let Some(progress) = self.notes_scrollbar().dragged(event, cx) else {
            return;
        };
        self.notes_scrollbar.hold();
        scroll_to(&self.notes_scroll, ScrollbarAxis::Vertical, progress);
        cx.notify();
    }

    /// Lets go of the thumb, and starts its clock again.
    fn release_scrollbar(&mut self, cx: &mut Context<Self>) {
        if let Some(epoch) = self.notes_scrollbar.release() {
            hide_later(epoch, cx, |dialog| Some(&mut dialog.notes_scrollbar));
            cx.notify();
        }
    }

    /// Puts the bar up while the pointer rests on the edge it rides, and starts
    /// it going the moment the pointer leaves.
    fn hover_scrollbar(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            if self.notes_scrollbar.hover_enter() {
                cx.notify();
            }
            return;
        }
        if let Some(epoch) = self.notes_scrollbar.hover_leave() {
            hide_now(self, epoch, cx, |dialog| Some(&mut dialog.notes_scrollbar));
        }
    }

    /// The release notes under their heading, scrolling past a few screens'
    /// worth, or `None` when the release has none to show.
    fn notes_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let blocks = self.notes()?;
        let theme = theme(cx);
        let mono = monospace_family(cx);
        let heading = label(cx, "update.notes");

        let list = div()
            .id("update-notes")
            .track_scroll(&self.notes_scroll)
            .flex()
            .flex_col()
            .min_h_0()
            .gap(px(6.))
            .max_h(px(NOTES_MAX_HEIGHT))
            .py(px(8.))
            .pl(px(10.))
            .pr(px(12.))
            .overflow_y_scroll()
            .smooth_scroll(&self.notes_scroll)
            .restrict_scroll_to_axis()
            .children(blocks.iter().map(|block| note_block(block, &theme, &mono)));

        let bar =
            self.notes_scrollbar()
                .on_hover(cx.listener(|dialog, hovered: &bool, _window, cx| {
                    dialog.hover_scrollbar(*hovered, cx);
                }));

        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .pt(px(4.))
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(heading),
                )
                .child(
                    // The middle box holds the overlay bar: a scrolling box
                    // cannot, because its children scroll away underneath it.
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .child(list)
                        .children(bar.render(&theme)),
                )
                .into_any_element(),
        )
    }

    /// The heading the panel wears in the current state.
    ///
    /// "Update available" is only true where there is one; the states a manual
    /// check can land in borrow the menu item's own wording instead, which is
    /// the question the user asked.
    fn title(&self, cx: &App) -> SharedString {
        match &self.state {
            State::Announce(_) | State::Busy { .. } => label(cx, "update.title"),
            State::Failed {
                release: Some(_), ..
            } => label(cx, "update.title"),
            _ => label(cx, "menu.check_updates"),
        }
    }

    /// The paragraph above the buttons.
    fn message(&self, cx: &mut Context<Self>) -> AnyElement {
        // Built first: it needs `cx` mutably, for its scrollbar's listener.
        let notes = self.notes_section(cx);
        let theme = theme(cx);
        let app = identity(cx);
        let column = div().flex().flex_col().gap(px(6.));
        let line =
            |words: SharedString| div().text_size(px(13.)).text_color(theme.text).child(words);
        let note = |words: SharedString| {
            div()
                .text_size(px(12.))
                .text_color(theme.text_muted)
                .child(words)
        };

        // The running version and the one on offer are named together, so the
        // user can see what the upgrade actually is. `%{app}` is offered
        // alongside `%{version}` for a host whose wording names itself; one
        // whose key does not mention it simply never sees it.
        let installed = text(
            cx,
            "update.installed",
            &[("app", app.name), ("version", app.version)],
        );
        let available = |release: &Release| {
            text(
                cx,
                "update.available",
                &[("app", app.name), ("version", &release.version)],
            )
        };

        match &self.state {
            // Never rendered: `render` leaves early while closed.
            State::Closed => div().into_any_element(),
            State::Checking => column
                .child(line(label(cx, "update.checking")))
                .into_any_element(),
            State::UpToDate => column
                .child(line(label(cx, "update.up_to_date")))
                .child(note(installed))
                .into_any_element(),
            State::Announce(release) => column
                .child(line(available(release)))
                .child(note(installed))
                .children(notes)
                .into_any_element(),
            State::Busy { release, phase } => {
                let (heading, fraction, share) = match phase {
                    Phase::Downloading { done, total } => {
                        let percent = percent(*done, *total);
                        (
                            label(cx, "update.downloading"),
                            f32::from(percent) / 100.,
                            Some(SharedString::from(format!("{percent}%"))),
                        )
                    }
                    // A swap is a handful of renames and an unpack; there is no
                    // honest fraction to draw, so the bar is left full and the
                    // wording carries the state.
                    Phase::Installing => (label(cx, "update.installing"), 1., None),
                };
                column
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .gap(px(8.))
                            .child(line(heading))
                            .children(share.map(note)),
                    )
                    .child(note(available(release)))
                    // A hairline track in the border color, so an idle-looking
                    // bar reads as part of the frame, with only the accent fill
                    // claiming attention.
                    .child(
                        div()
                            .flex_none()
                            .w_full()
                            .h(px(PROGRESS_BAR))
                            .rounded_sm()
                            .bg(theme.border)
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(fraction))
                                    .rounded_sm()
                                    .bg(theme.accent),
                            ),
                    )
                    .into_any_element()
            }
            State::Failed { message, .. } => column
                .child(line(label(cx, "update.failed")))
                .child(note(SharedString::from(message.clone())))
                .into_any_element(),
        }
    }

    /// The button row for the current state.
    fn buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let row = div().flex().flex_row().justify_end().gap(px(8.));

        let close = |id: &'static str, words: SharedString, variant: ButtonVariant| {
            let this = this.clone();
            Button::new(id, words)
                .variant(variant)
                .on_click(move |_, _window, cx| {
                    this.update(cx, |dialog, cx| dialog.dismiss(cx));
                })
        };

        match &self.state {
            State::Closed => div().into_any_element(),
            // A check in flight can be walked away from; the answer is dropped
            // when it lands, rather than reopening a dialog the user closed.
            State::Checking => row
                .child(close(
                    "update-cancel",
                    label(cx, "common.cancel"),
                    ButtonVariant::Secondary,
                ))
                .into_any_element(),
            State::UpToDate => row
                .child(close(
                    "update-close",
                    label(cx, "common.close"),
                    ButtonVariant::Primary,
                ))
                .into_any_element(),
            // Cancel first, then the two commitments, with the recommended one
            // last and filled: the same left-to-right ordering the other
            // dialogs use for "back out" against "go ahead".
            State::Announce(release) => row
                .child(close(
                    "update-cancel",
                    label(cx, "common.cancel"),
                    ButtonVariant::Secondary,
                ))
                .child(
                    Button::new("update-ignore", label(cx, "update.ignore"))
                        .variant(ButtonVariant::Secondary)
                        .on_click({
                            let this = this.clone();
                            let release = release.clone();
                            move |_, _window, cx| {
                                let release = release.clone();
                                this.update(cx, |dialog, cx| dialog.ignore(&release, cx));
                            }
                        }),
                )
                .child(
                    Button::new("update-install", label(cx, "update.update"))
                        .variant(ButtonVariant::Primary)
                        .on_click({
                            let this = this.clone();
                            let release = release.clone();
                            move |_, _window, cx| {
                                let release = release.clone();
                                this.update(cx, |dialog, cx| {
                                    if release.asset.is_some() {
                                        dialog.install(release, cx);
                                    } else {
                                        // Nothing published for this platform:
                                        // hand off to the browser, as this
                                        // dialog always used to.
                                        dialog.open_page(&release, cx);
                                    }
                                });
                            }
                        }),
                )
                .into_any_element(),
            // No way out of an install but the end of it.
            State::Busy { .. } => div().into_any_element(),
            State::Failed { release, .. } => row
                .children(release.as_ref().map(|release| {
                    let this = this.clone();
                    let release = release.clone();
                    Button::new("update-page", label(cx, "update.open_release"))
                        .variant(ButtonVariant::Secondary)
                        .on_click(move |_, _window, cx| {
                            let release = release.clone();
                            this.update(cx, |dialog, cx| dialog.open_page(&release, cx));
                        })
                }))
                .child(close(
                    "update-close",
                    label(cx, "common.close"),
                    ButtonVariant::Primary,
                ))
                .into_any_element(),
        }
    }
}

/// How far along a download is, as a whole percent.
///
/// A release that reported no size has no fraction to show; treating it as
/// finished would draw a full bar over a download that has not started, so it
/// reads as zero until the bytes stop arriving.
fn percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    // The early return is not only the "more arrived than promised" case: it is
    // also what keeps the multiplication below from saturating, which on an
    // implausibly large byte count would answer 1% for a finished download.
    if done >= total {
        return 100;
    }
    let percent = done.saturating_mul(100) / total;
    u8::try_from(percent.min(100)).unwrap_or(100)
}

/// One block of release notes, drawn in the dialog's small type.
fn note_block(block: &Block, theme: &Theme, mono: &SharedString) -> AnyElement {
    let words = |inline: &Inline| styled(inline, theme, mono);
    match block {
        Block::Heading(inline) => div()
            .pt(px(4.))
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .child(words(inline))
            .into_any_element(),
        Block::Paragraph(inline) => div()
            .text_size(px(12.))
            .text_color(theme.text)
            .child(words(inline))
            .into_any_element(),
        Block::Item(inline) => div()
            .flex()
            .flex_row()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(theme.text)
            .child(div().flex_none().text_color(theme.text_muted).child("•"))
            .child(div().flex_1().min_w_0().child(words(inline)))
            .into_any_element(),
    }
}

/// `inline` as styled text: strong spans in a heavier weight, code spans in
/// the monospace face on a faint wash.
fn styled(inline: &Inline, theme: &Theme, mono: &SharedString) -> StyledText {
    let strong = inline.strong.iter().map(|range| {
        (
            range.clone(),
            HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                ..HighlightStyle::default()
            },
        )
    });
    let code = inline.code.iter().map(|range| {
        (
            range.clone(),
            HighlightStyle {
                background_color: Some(theme.surface_active),
                ..HighlightStyle::default()
            },
        )
    });
    let mut highlights: Vec<_> = strong.chain(code).collect();
    // `StyledText` wants its highlights in order and merges overlapping ones.
    highlights.sort_by_key(|(range, _)| range.start);
    StyledText::new(inline.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(
            inline
                .code
                .iter()
                .map(|range| (range.clone(), mono.clone())),
        )
}

impl EventEmitter<UpdateDialogEvent> for UpdateDialog {}

impl Focusable for UpdateDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for UpdateDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open() {
            return div().id("update-dialog");
        }

        self.apply_pending_focus(window, cx);
        self.watch_scroll(cx);

        let width = if self.notes().is_some() {
            NOTES_WIDTH
        } else {
            DIALOG_WIDTH
        };
        let title = self.title(cx);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(self.message(cx))
            .child(self.buttons(cx));

        let on_dismiss = {
            let this = cx.entity();
            move |_window: &mut Window, cx: &mut App| {
                this.update(cx, |dialog, cx| dialog.dismiss(cx));
            }
        };

        // Absolute and full-size for the same reason as the other dialogs: an
        // absolutely positioned child is laid out against its direct parent.
        div()
            .id("update-dialog")
            .absolute()
            .inset_0()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_drag_move::<DraggedThumb>(cx.listener(
                |dialog, event: &DragMoveEvent<DraggedThumb>, _window, cx| {
                    dialog.drag_scrollbar(event, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|dialog, _: &MouseUpEvent, _window, cx| {
                    dialog.release_scrollbar(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|dialog, _: &MouseUpEvent, _window, cx| {
                    dialog.release_scrollbar(cx);
                }),
            )
            .child(modal("update-modal", title, px(width), body, on_dismiss))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_percentage_covers_both_ends_and_the_unknown_size() {
        assert_eq!(percent(0, 1000), 0);
        assert_eq!(percent(500, 1000), 50);
        assert_eq!(percent(1000, 1000), 100);
        // More than promised is still finished, not 240%.
        assert_eq!(percent(2400, 1000), 100);
        // A release that named no size cannot be drawn as complete.
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(9_999, 0), 0);
        // The multiplication has to survive an implausible byte count.
        assert_eq!(percent(u64::MAX, u64::MAX), 100);
    }
}
