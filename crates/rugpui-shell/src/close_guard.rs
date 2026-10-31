//! Asking before a close throws work away.
//!
//! Four gestures end what a window holds, and until now every one of them went
//! straight through: the caption's close button removed the window, the
//! platform's own close — the title bar the platform draws, Alt+F4, the dock —
//! closed it, Quit called `cx.quit()` and an installed update called
//! `cx.restart()`. An application that guarded closing a single tab had nothing
//! to guard closing all of them with.
//!
//! This module is the one path the four share. The application installs a
//! guard per window with [`set_close_guard`] — a function that is told what is
//! about to happen ([`CloseIntent`]) and answers whether it may go ahead now —
//! and routes its gestures through it:
//!
//! | Gesture | Through |
//! |---|---|
//! | The caption's close button | [`request_close`] — the strips [`crate::window_control_strips`] builds already call it |
//! | The platform's close | the guard itself, which [`set_close_guard`] registers with `on_window_should_close` |
//! | Quit (Ctrl+Q, Cmd+Q, the menu) | [`request_quit`], from the `Quit` action handler |
//! | The restart after an update | [`request_restart`], from `UpdateDialogEvent::Installed` |
//!
//! A guard that answers `false` has taken the decision over: it asks the user,
//! and once the user has said to go on it calls [`proceed`] with the same
//! intent, which performs it without asking again. [`CloseGuard`] is that guard
//! ready-made, for an application whose answer is a list of what would be
//! lost: it asks with [`UnsavedWorkDialog`] when the list is not empty and not
//! at all when it is.
//!
//! ```ignore
//! // In the workspace's constructor:
//! let workspace = cx.weak_entity();
//! self.close_guard = cx.new(|cx| {
//!     CloseGuard::new(window, cx, move |_intent, _window, cx| {
//!         workspace.read_with(cx, |this, cx| this.unsaved_work(cx)).unwrap_or_default()
//!     })
//! });
//! // At the end of the root element, after every other overlay:
//! root.child(self.close_guard.clone())
//! // Once, at start-up:
//! cx.on_action(|_: &Quit, cx| rugpui_shell::request_quit(cx));
//! ```

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyWindowHandle, App, Context, FocusHandle, Global, SharedString, Task, Window, WindowId, div,
    prelude::*, px,
};
use rugpui::{Button, ButtonVariant, TextSize, modal, theme};

use crate::inject::{label, text};

/// What a close is about to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseIntent {
    /// Close this window — the caption's button or the platform's close.
    Window,
    /// Quit the application, every window with it.
    Quit,
    /// Restart the application, after an update has been installed.
    Restart,
}

/// What the user answered [`UnsavedWorkDialog`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    /// Keep everything open; nothing happens.
    Cancel,
    /// Go on, and lose what was listed.
    Discard,
    /// Save everything first, and go on once that has worked. Only offered
    /// when the dialog was told the application can.
    SaveAll,
}

/// A window's guard; see [`set_close_guard`].
type Guard = Rc<dyn Fn(CloseIntent, &mut Window, &mut App) -> bool>;

/// Every installed guard, by the window it guards.
#[derive(Default)]
struct CloseGuards(HashMap<WindowId, Guard>);

impl Global for CloseGuards {}

/// Installs `guard` for `window`, replacing any guard it had.
///
/// `guard` is told what is about to happen and answers whether it may happen
/// now. Answering `false` holds it: the guard then asks the user in its own
/// way, and calls [`proceed`] with the same intent once the answer is to go on.
/// It is also what the platform's own close asks — the title bar the platform
/// draws, Alt+F4, the window menu — through `on_window_should_close`.
///
/// The guard may be asked from inside a platform callback, so it should answer
/// from state it can read, and leave anything slow to the question it asks.
pub fn set_close_guard(
    window: &mut Window,
    cx: &mut App,
    guard: impl Fn(CloseIntent, &mut Window, &mut App) -> bool + 'static,
) {
    let id = window.window_handle().window_id();
    cx.default_global::<CloseGuards>()
        .0
        .insert(id, Rc::new(guard));
    window.on_window_should_close(cx, move |window, cx| {
        ask(id, CloseIntent::Window, window, cx)
    });
}

/// Takes `window`'s guard away, so that every gesture goes straight through
/// again.
pub fn clear_close_guard(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if let Some(guards) = cx.try_global::<CloseGuards>()
        && guards.0.contains_key(&id)
    {
        cx.global_mut::<CloseGuards>().0.remove(&id);
    }
}

/// The guard of window `id`, if it has one.
fn guard(id: WindowId, cx: &App) -> Option<Guard> {
    cx.try_global::<CloseGuards>()?.0.get(&id).cloned()
}

/// Whether window `id`'s guard lets `intent` go ahead now; `true` when the
/// window has no guard.
fn ask(id: WindowId, intent: CloseIntent, window: &mut Window, cx: &mut App) -> bool {
    match guard(id, cx) {
        Some(guard) => guard(intent, window, cx),
        None => true,
    }
}

/// Closes `window` once its guard agrees — what the caption's close button,
/// and any "Close window" command of the application's, call.
///
/// Deferred to the end of the current update, so it can be called from inside
/// an entity's own listener without the guard tripping over that entity.
pub fn request_close(window: &mut Window, cx: &mut App) {
    window.defer(cx, |window, cx| {
        let id = window.window_handle().window_id();
        if ask(id, CloseIntent::Window, window, cx) {
            proceed(CloseIntent::Window, window, cx);
        }
    });
}

/// Quits once every window's guard agrees — what the `Quit` action calls.
///
/// The active window is asked first, then the rest; the first guard that
/// holds the quit stops the round, and asks the user itself. A window with no
/// guard has nothing to say. Deferred, like [`request_close`], and runs with
/// no window of its own, so it can be called from an application-level action
/// handler.
pub fn request_quit(cx: &mut App) {
    cx.defer(|cx| {
        let mut windows: Vec<AnyWindowHandle> = cx.windows();
        if let Some(active) = cx.active_window() {
            windows.sort_by_key(|window| *window != active);
        }
        for handle in windows {
            let id = handle.window_id();
            if guard(id, cx).is_none() {
                continue;
            }
            let go_on = handle
                .update(cx, |_, window, cx| ask(id, CloseIntent::Quit, window, cx))
                .unwrap_or(true);
            if !go_on {
                return;
            }
        }
        cx.quit();
    });
}

/// Restarts into the installed update once `window`'s guard agrees — what
/// `UpdateDialogEvent::Installed` should call instead of `cx.restart()`.
pub fn request_restart(window: &mut Window, cx: &mut App) {
    window.defer(cx, |window, cx| {
        let id = window.window_handle().window_id();
        if ask(id, CloseIntent::Restart, window, cx) {
            proceed(CloseIntent::Restart, window, cx);
        }
    });
}

/// Performs `intent` without asking anyone: the call a guard makes once the
/// user has said to go on.
///
/// [`CloseIntent::Window`] removes the window, and its guard with it.
/// [`CloseIntent::Quit`] quits. [`CloseIntent::Restart`] restarts with the
/// path [`crate::restart_path`] recorded, for the reason that function gives.
pub fn proceed(intent: CloseIntent, window: &mut Window, cx: &mut App) {
    match intent {
        CloseIntent::Window => {
            clear_close_guard(window, cx);
            window.remove_window();
        }
        CloseIntent::Quit => cx.quit(),
        CloseIntent::Restart => {
            if let Some(path) = crate::restart_path() {
                cx.set_restart_path(path);
            }
            cx.restart();
        }
    }
}

/// How many items [`UnsavedWorkDialog`] lists before it says how many more
/// there are: enough to recognize the work by, few enough that the buttons are
/// still on screen.
const LISTED: usize = 8;

/// Callback receiving the user's answer.
type ChoiceHandler = Rc<dyn Fn(CloseChoice, &mut Window, &mut App)>;

/// The question a close asks when it would lose work: what would be lost, and
/// Cancel, the gesture "anyway" and, when the application can, Save all.
///
/// A stateless element like the widgets it is built from, rendered by the
/// view that owns the question — [`CloseGuard`] does — as the last child of a
/// window-sized `relative()` root. Cancel is the default: it has the focus as
/// the dialog opens when [`UnsavedWorkDialog::cancel_focus`] hands it a
/// handle, and `Escape` and the backdrop answer it too.
///
/// The words are the application's, looked up through [`crate::Strings`]:
///
/// | Key | Where |
/// |---|---|
/// | `close_guard.title_window` `close_guard.title_quit` `close_guard.title_restart` | The title, per [`CloseIntent`] |
/// | `close_guard.message` | The line above the list |
/// | `close_guard.more` | Under a list cut short; `%{count}` is how many were left out |
/// | `close_guard.close_anyway` `close_guard.quit_anyway` `close_guard.restart_anyway` | The button that goes on, per [`CloseIntent`] |
/// | `close_guard.save_all` | The Save all button |
/// | `common.cancel` | Cancel |
#[derive(IntoElement)]
pub struct UnsavedWorkDialog {
    intent: CloseIntent,
    items: Vec<SharedString>,
    save_all: bool,
    busy: bool,
    cancel_focus: Option<FocusHandle>,
    on_choice: Option<ChoiceHandler>,
}

impl UnsavedWorkDialog {
    /// The question for `intent`, listing `items` — one line each, in the
    /// application's words: "orders.sql", "Template customer.ftl", "An
    /// uncommitted transaction on reporting".
    pub fn new(intent: CloseIntent, items: impl IntoIterator<Item = SharedString>) -> Self {
        Self {
            intent,
            items: items.into_iter().collect(),
            save_all: false,
            busy: false,
            cancel_focus: None,
            on_choice: None,
        }
    }

    /// Offers Save all beside the other two.
    pub fn save_all(mut self, offered: bool) -> Self {
        self.save_all = offered;
        self
    }

    /// Grays the buttons out while a Save all is running.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// Focuses the Cancel button, tracked by `handle`, as the dialog opens —
    /// so `Enter` is Cancel, which is the answer that loses nothing.
    pub fn cancel_focus(mut self, handle: &FocusHandle) -> Self {
        self.cancel_focus = Some(handle.clone());
        self
    }

    /// Called with the answer.
    pub fn on_choice(
        mut self,
        handler: impl Fn(CloseChoice, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_choice = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for UnsavedWorkDialog {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = theme(cx);
        let (title, anyway) = match self.intent {
            CloseIntent::Window => ("close_guard.title_window", "close_guard.close_anyway"),
            CloseIntent::Quit => ("close_guard.title_quit", "close_guard.quit_anyway"),
            CloseIntent::Restart => ("close_guard.title_restart", "close_guard.restart_anyway"),
        };
        let choose = {
            let handler = self.on_choice.clone();
            move |choice: CloseChoice| {
                let handler = handler.clone();
                move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                    if let Some(handler) = &handler {
                        handler(choice, window, cx);
                    }
                }
            }
        };
        let dismiss = {
            let handler = self.on_choice.clone();
            move |window: &mut Window, cx: &mut App| {
                if let Some(handler) = &handler {
                    handler(CloseChoice::Cancel, window, cx);
                }
            }
        };

        let left_out = self.items.len().saturating_sub(LISTED);
        let list = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .pl(px(8.))
            .text_size(TextSize::Body.size(cx))
            .children(
                self.items
                    .iter()
                    .take(LISTED)
                    .map(|item| div().child(format!("\u{2022} {item}"))),
            )
            .when(left_out > 0, |list| {
                list.child(div().text_color(palette.text_muted).child(text(
                    cx,
                    "close_guard.more",
                    &[("count", &left_out.to_string())],
                )))
            });

        let cancel = Button::new("unsaved-work-cancel", label(cx, "common.cancel"))
            .variant(ButtonVariant::Secondary)
            .disabled(self.busy)
            .on_click(choose(CloseChoice::Cancel));
        let cancel = match &self.cancel_focus {
            Some(handle) => cancel.focus_handle(handle),
            None => cancel,
        };
        let buttons = div()
            .flex()
            .flex_row()
            .justify_end()
            .gap(px(8.))
            .pt(px(4.))
            .when(self.save_all, |row| {
                row.child(
                    Button::new("unsaved-work-save", label(cx, "close_guard.save_all"))
                        .variant(ButtonVariant::Primary)
                        .disabled(self.busy)
                        .on_click(choose(CloseChoice::SaveAll)),
                )
            })
            .child(cancel)
            .child(
                Button::new("unsaved-work-discard", label(cx, anyway))
                    .variant(ButtonVariant::Danger)
                    .disabled(self.busy)
                    .on_click(choose(CloseChoice::Discard)),
            );

        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(TextSize::Body.size(cx))
                    .child(label(cx, "close_guard.message")),
            )
            .child(list)
            .child(buttons);

        let dialog = modal(
            "unsaved-work",
            label(cx, title),
            rugpui::scaled(440., cx),
            body,
            dismiss,
        )
        .escape_dismisses(!self.busy)
        .backdrop_dismisses(!self.busy);
        match &self.cancel_focus {
            Some(handle) => dialog.initial_focus(handle),
            None => dialog.autofocus(true),
        }
    }
}

/// Saves everything the guard listed; resolves to whether it all worked.
type SaveAll = Rc<dyn Fn(&mut Window, &mut App) -> Task<bool>>;

/// What [`CloseGuard`] asks the application for.
type Unsaved = Rc<dyn Fn(CloseIntent, &mut Window, &mut App) -> Vec<SharedString>>;

/// The guard most applications want, ready-made: it asks the application what
/// `intent` would lose, lets the close through when the answer is nothing, and
/// otherwise asks the user with [`UnsavedWorkDialog`] and goes on — or does
/// not — according to the answer.
///
/// An entity, because the question has to be drawn somewhere: render it as the
/// last child of the window's root, after every other overlay, so that the
/// question is on top of whatever else is open. It draws nothing while nothing
/// is being asked.
pub struct CloseGuard {
    unsaved: Unsaved,
    save_all: Option<SaveAll>,
    pending: Option<(CloseIntent, Vec<SharedString>)>,
    saving: bool,
    cancel: FocusHandle,
    _saving: Option<Task<()>>,
}

impl CloseGuard {
    /// Installs itself as `window`'s guard, asking `unsaved` what a close
    /// would lose. Each line of the answer is one item of the question.
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        unsaved: impl Fn(CloseIntent, &mut Window, &mut App) -> Vec<SharedString> + 'static,
    ) -> Self {
        let this = cx.weak_entity();
        set_close_guard(window, cx, move |intent, window, cx| {
            this.update(cx, |guard, cx| guard.request(intent, window, cx))
                .unwrap_or(true)
        });
        Self {
            unsaved: Rc::new(unsaved),
            save_all: None,
            pending: None,
            saving: false,
            cancel: cx.focus_handle(),
            _saving: None,
        }
    }

    /// Offers Save all, run by `save`: the close goes on once the task it
    /// returns resolves to `true` and nothing is left to lose. On `false`, or
    /// with work still listed, the question stays up with what is left.
    pub fn with_save_all(
        mut self,
        save: impl Fn(&mut Window, &mut App) -> Task<bool> + 'static,
    ) -> Self {
        self.save_all = Some(Rc::new(save));
        self
    }

    /// Whether the question is on screen.
    pub fn is_open(&self) -> bool {
        self.pending.is_some()
    }

    /// Asks about `intent`: `true` when nothing would be lost and the caller
    /// may go on, `false` when the question is now up and the guard will
    /// [`proceed`] itself if the user says to.
    pub fn request(
        &mut self,
        intent: CloseIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.pending.is_some() {
            // Already asking; a second close while the question is up waits
            // for its answer rather than stacking another.
            return false;
        }
        let items = (self.unsaved)(intent, window, cx);
        if items.is_empty() {
            return true;
        }
        self.pending = Some((intent, items));
        cx.notify();
        false
    }

    /// Acts on the user's answer.
    fn choose(&mut self, choice: CloseChoice, window: &mut Window, cx: &mut Context<Self>) {
        let Some((intent, _)) = self.pending.clone() else {
            return;
        };
        match choice {
            CloseChoice::Cancel => {
                self.pending = None;
                cx.notify();
            }
            CloseChoice::Discard => {
                self.pending = None;
                cx.notify();
                proceed(intent, window, cx);
            }
            CloseChoice::SaveAll => {
                let Some(save) = self.save_all.clone() else {
                    return;
                };
                self.saving = true;
                cx.notify();
                let saved = save(window, cx);
                self._saving = Some(cx.spawn_in(window, async move |this, cx| {
                    let ok = saved.await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.saving = false;
                        let left = (this.unsaved)(intent, window, cx);
                        if ok && left.is_empty() {
                            this.pending = None;
                            proceed(intent, window, cx);
                        } else if left.is_empty() {
                            this.pending = None;
                        } else {
                            this.pending = Some((intent, left));
                        }
                        cx.notify();
                    });
                }));
            }
        }
    }
}

impl Render for CloseGuard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((intent, items)) = self.pending.clone() else {
            // Out of the flow, so the root's gaps and flex do not see it.
            return div().absolute().into_any_element();
        };
        let this = cx.weak_entity();
        UnsavedWorkDialog::new(intent, items)
            .save_all(self.save_all.is_some())
            .busy(self.saving)
            .cancel_focus(&self.cancel)
            .on_choice(move |choice, window, cx| {
                let _ = this.update(cx, |guard, cx| guard.choose(choice, window, cx));
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use gpui::{Entity, TestAppContext, VisualTestContext};

    use super::*;

    /// A window root that holds a [`CloseGuard`] over a list of work the test
    /// controls.
    struct Root {
        guard: Entity<CloseGuard>,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().relative().size_full().child(self.guard.clone())
        }
    }

    fn open(
        cx: &mut TestAppContext,
        work: Rc<RefCell<Vec<SharedString>>>,
        asked: Rc<RefCell<Vec<CloseIntent>>>,
    ) -> (Entity<Root>, VisualTestContext) {
        cx.update(rugpui::init);
        let window = cx.add_window(move |window, cx| {
            let guard = cx.new(|cx| {
                CloseGuard::new(window, cx, move |intent, _, _| {
                    asked.borrow_mut().push(intent);
                    work.borrow().clone()
                })
            });
            Root { guard }
        });
        let root = window.root(cx).unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx);
        (root, cx)
    }

    /// With nothing to lose the platform's close goes through; with work it
    /// is held and the question goes up, and Cancel takes it down again.
    #[gpui::test]
    fn the_platform_close_asks_only_when_there_is_work(cx: &mut TestAppContext) {
        let work = Rc::new(RefCell::new(Vec::new()));
        let asked = Rc::new(RefCell::new(Vec::new()));
        let (root, mut cx) = open(cx, work.clone(), asked.clone());
        cx.run_until_parked();

        assert!(cx.simulate_close(), "nothing to lose, so it closes");
        work.borrow_mut().push("orders.sql".into());
        assert!(!cx.simulate_close(), "work is held");
        assert_eq!(*asked.borrow(), [CloseIntent::Window, CloseIntent::Window]);
        let guard = root.read_with(&cx, |root, _| root.guard.clone());
        assert!(guard.read_with(&cx, |guard, _| guard.is_open()));

        // A second close while the question is up waits for its answer.
        assert!(!cx.simulate_close());
        assert_eq!(asked.borrow().len(), 2);

        cx.update(|window, cx| {
            guard.update(cx, |guard, cx| {
                guard.choose(CloseChoice::Cancel, window, cx)
            })
        });
        assert!(!guard.read_with(&cx, |guard, _| guard.is_open()));
    }

    /// "Anyway" performs what was asked about, without asking again.
    #[gpui::test]
    fn discarding_closes_the_window_without_asking_again(cx: &mut TestAppContext) {
        let work = Rc::new(RefCell::new(vec![SharedString::from("Template a.ftl")]));
        let asked = Rc::new(RefCell::new(Vec::new()));
        let (root, mut cx) = open(cx, work, asked.clone());
        cx.run_until_parked();

        cx.update(request_close);
        cx.run_until_parked();
        let guard = root.read_with(&cx, |root, _| root.guard.clone());
        assert!(guard.read_with(&cx, |guard, _| guard.is_open()));

        let closed = Rc::new(Cell::new(false));
        cx.update(|window, cx| {
            let closed = closed.clone();
            cx.on_window_closed(move |_, _| closed.set(true)).detach();
            guard.update(cx, |guard, cx| {
                guard.choose(CloseChoice::Discard, window, cx)
            });
        });
        cx.run_until_parked();
        assert!(closed.get(), "the window went");
        assert_eq!(*asked.borrow(), [CloseIntent::Window]);
    }

    /// A guard installed by hand decides alone, and taking it away lets the
    /// close straight through.
    #[gpui::test]
    fn a_hand_installed_guard_can_be_taken_away(cx: &mut TestAppContext) {
        cx.update(rugpui::init);
        let window = cx.add_window(|_, _| Plain);
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        let calls = Rc::new(Cell::new(0));
        cx.update(|window, cx| {
            let calls = calls.clone();
            set_close_guard(window, cx, move |intent, _, _| {
                assert_eq!(intent, CloseIntent::Window);
                calls.set(calls.get() + 1);
                false
            });
        });
        assert!(!cx.simulate_close());
        assert_eq!(calls.get(), 1);
        cx.update(|window, cx| clear_close_guard(window, cx));
        assert!(cx.simulate_close());
        assert_eq!(calls.get(), 1);
    }

    struct Plain;

    impl Render for Plain {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
}
