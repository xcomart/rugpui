//! Keyboard and scrolling state shared by the two dropdown controls.

use std::time::{Duration, Instant};

use gpui::{
    App, Div, ElementId, Entity, FocusHandle, KeyBinding, KeyDownEvent, ScrollHandle, Subscription,
    Window, actions,
};

use crate::scrollbar::{
    Scrollbar, ScrollbarAxis, ScrollbarState, answer_own_scrollbar, own_scrollbar,
};

pub(crate) const KEY_CONTEXT: &str = "RugpuiSelect";

actions!(rugpui_select, [Dismiss]);

pub(crate) fn init(cx: &mut App) {
    // Bind at the focused dropdown: host actions run before raw key listeners.
    cx.bind_keys([KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT))]);
}

const TYPEAHEAD_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Default)]
pub(crate) struct Typeahead {
    query: String,
    last_input: Option<Instant>,
}

impl Typeahead {
    pub(crate) fn clear(&mut self) {
        self.query.clear();
        self.last_input = None;
    }

    fn expire(&mut self, now: Instant) {
        if self
            .last_input
            .is_some_and(|last| now.duration_since(last) >= TYPEAHEAD_TIMEOUT)
        {
            self.clear();
        }
    }

    pub(crate) fn find<'a>(
        &mut self,
        text: &str,
        current: Option<usize>,
        count: usize,
        label: impl Fn(usize) -> &'a str,
        now: Instant,
    ) -> Option<usize> {
        self.expire(now);
        let continuing = !self.query.is_empty();
        self.query.push_str(&text.to_lowercase());
        self.last_input = Some(now);

        let find = |prefix: &str, start| {
            (0..count)
                .map(|offset| (start + offset) % count)
                .find(|&index| label(index).to_lowercase().starts_with(prefix))
        };
        let start = current.map_or(0, |index| if continuing { index } else { index + 1 });
        if let Some(index) = find(&self.query, start) {
            return Some(index);
        }

        // Repeated letters cycle when the full prefix has no match. Keep a
        // real doubled-letter prefix ("aa" in "Aardvark") searchable too.
        let first = self.query.chars().next()?;
        if self.query.chars().all(|ch| ch == first) {
            self.query = first.to_string();
            return find(&self.query, current.map_or(0, |index| index + 1));
        }
        None
    }
}

pub(crate) struct SelectKeyboard {
    search: Typeahead,
    open: bool,
    pub(crate) focus: FocusHandle,
    pub(crate) scroll: ScrollHandle,
    /// The open list's own overlay bar, for an owner that draws none.
    scrollbar: ScrollbarState,
    _blur: Subscription,
}

/// Id of the bar a dropdown draws over its own list.
fn bar_id(id: &ElementId) -> ElementId {
    ElementId::from((id.clone(), "scrollbar"))
}

impl SelectKeyboard {
    pub(crate) fn state(
        id: ElementId,
        open: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let state = window.use_keyed_state((id, "keyboard"), cx, |window, cx| {
            let focus = cx.focus_handle();
            let blur = cx.on_blur(&focus, window, |this: &mut Self, _, _| this.search.clear());
            Self {
                search: Typeahead::default(),
                open,
                focus,
                scroll: ScrollHandle::new(),
                scrollbar: ScrollbarState::new(),
                _blur: blur,
            }
        });
        state.update(cx, |state, _| {
            if state.open != open {
                state.search.clear();
                state.open = open;
            }
        });
        state
    }

    /// `None` leaves the key to the host; `Some(None)` consumes an unmatched
    /// prefix without changing the selection.
    pub(crate) fn navigate<'a>(
        &mut self,
        event: &KeyDownEvent,
        current: Option<usize>,
        count: usize,
        label: impl Fn(usize) -> &'a str,
        now: Instant,
    ) -> Option<Option<usize>> {
        let stroke = &event.keystroke;
        if stroke.modifiers.control
            || stroke.modifiers.alt
            || stroke.modifiers.platform
            || stroke.modifiers.function
        {
            self.search.clear();
            return None;
        }
        self.search.expire(now);
        if matches!(stroke.key.as_str(), "up" | "down") {
            self.search.clear();
            if !self.open || stroke.modifiers.modified() || count == 0 {
                return None;
            }
            let last = count - 1;
            let next = match (current, stroke.key.as_str()) {
                (Some(index), "up") => index.saturating_sub(1),
                (Some(index), _) => (index + 1).min(last),
                (None, "up") => last,
                (None, _) => 0,
            };
            return Some(Some(next));
        }
        // Space still activates an idle trigger; inside a prefix it is text.
        if stroke.key == "space" && self.search.query.is_empty() {
            return None;
        }
        let text = stroke.key_char.as_deref().unwrap_or_else(|| {
            if stroke.key == "space" {
                " "
            } else if stroke.key.chars().count() == 1 {
                &stroke.key
            } else {
                ""
            }
        });
        if text.is_empty() || text.chars().any(char::is_control) {
            self.search.clear();
            return None;
        }
        Some(self.search.find(text, current, count, label, now))
    }

    /// The open list's overlay bar, for an owner that brought none of its own.
    ///
    /// Run off this per-dropdown state through [`own_scrollbar`], so a list
    /// scrolls with the same bar as every other surface without its owner
    /// keeping a [`ScrollbarState`], a timer and a drag listener for it.
    pub(crate) fn scrollbar(
        state: &Entity<Self>,
        id: &ElementId,
        handle: &ScrollHandle,
        cx: &mut App,
    ) -> Scrollbar {
        own_scrollbar(
            state,
            |this| &mut this.scrollbar,
            bar_id(id),
            ScrollbarAxis::Vertical,
            handle,
            cx,
        )
    }

    /// Has `list`, the box the bar from [`SelectKeyboard::scrollbar`] is drawn
    /// in, answer drags of that bar and the release that ends one.
    pub(crate) fn answer_scrollbar(
        state: &Entity<Self>,
        id: &ElementId,
        handle: &ScrollHandle,
        list: Div,
    ) -> Div {
        answer_own_scrollbar(
            list,
            state,
            |this| &mut this.scrollbar,
            bar_id(id),
            ScrollbarAxis::Vertical,
            handle,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SchemeSelect, SchemeSwatch, Select};
    use gpui::{
        Context, Modifiers, MouseButton, Render, TestAppContext, VisualTestContext, div, point,
        prelude::*, px, size,
    };
    use std::ops::Deref;

    actions!(select_keyboard_test, [DismissDialog]);

    #[test]
    fn prefixes_refine_cycle_wrap_and_expire() {
        let labels = ["Alpha", "Alpine", "Beta", "Betamax", "Éclair", "가나다"];
        let now = Instant::now();
        let mut search = Typeahead::default();
        let mut find = |text, current, elapsed| {
            search.find(text, current, labels.len(), |i| labels[i], now + elapsed)
        };
        assert_eq!(find("B", Some(0), Duration::ZERO), Some(2));
        assert_eq!(find("e", Some(2), Duration::ZERO), Some(2));
        assert_eq!(find("tam", Some(2), Duration::ZERO), Some(3));
        assert_eq!(find("z", Some(3), Duration::ZERO), None);
        assert_eq!(find("a", Some(3), TYPEAHEAD_TIMEOUT), Some(0));
        assert_eq!(find("a", Some(0), TYPEAHEAD_TIMEOUT), Some(1));
        assert_eq!(find("a", Some(1), TYPEAHEAD_TIMEOUT), Some(0));
        assert_eq!(find("É", Some(0), TYPEAHEAD_TIMEOUT * 2), Some(4));
        assert_eq!(find("가", None, TYPEAHEAD_TIMEOUT * 3), Some(5));
        assert_eq!(find("나", Some(5), TYPEAHEAD_TIMEOUT * 3), Some(5));
    }

    #[test]
    fn empty_options_have_no_match() {
        assert_eq!(
            Typeahead::default().find("a", None, 0, |_| "", Instant::now()),
            None
        );
    }

    #[test]
    fn repeated_letters_can_form_a_real_prefix() {
        let labels = ["Abacus", "Aardvark", "Alpine"];
        let mut search = Typeahead::default();
        let now = Instant::now();
        assert_eq!(search.find("a", None, 3, |i| labels[i], now), Some(0));
        assert_eq!(search.find("a", Some(0), 3, |i| labels[i], now), Some(1));
        assert_eq!(search.find("r", Some(1), 3, |i| labels[i], now), Some(1));
    }

    struct Harness {
        scheme: bool,
        open: bool,
        disabled: bool,
        selected: usize,
        options: Vec<String>,
        scroll: ScrollHandle,
        changes: Vec<usize>,
        dialog_dismissals: usize,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let entity = cx.entity();
            let on_open_change = move |open, _: &mut Window, cx: &mut App| {
                entity.update(cx, |this, cx| {
                    this.open = open;
                    cx.notify();
                });
            };
            let control = if self.scheme {
                SchemeSelect::new("picker")
                    .options(
                        self.options
                            .iter()
                            .enumerate()
                            .map(|(i, name)| SchemeSwatch::new(format!("id-{i}"), name.clone())),
                    )
                    .selected(Some(format!("id-{}", self.selected)))
                    .open(self.open)
                    .disabled(self.disabled)
                    .scroll_handle(self.scroll.clone())
                    .on_select(cx.listener(|this, id: &str, _, cx| {
                        let i = id.strip_prefix("id-").unwrap().parse().unwrap();
                        this.selected = i;
                        this.changes.push(i);
                        cx.notify();
                    }))
                    .on_open_change(on_open_change)
                    .into_any_element()
            } else {
                let entity = cx.entity();
                Select::new("picker")
                    .options(self.options.clone())
                    .selected(Some(self.options[self.selected].clone()))
                    .open(self.open)
                    .scroll_handle(self.scroll.clone())
                    .on_select(move |i, _, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.selected = i;
                            this.changes.push(i);
                            cx.notify();
                        });
                    })
                    .on_open_change(on_open_change)
                    .into_any_element()
            };
            div()
                .size_full()
                .key_context("SelectTestDialog")
                .on_action(cx.listener(|this, _: &DismissDialog, _, cx| {
                    this.dialog_dismissals += 1;
                    cx.stop_propagation();
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        this.dialog_dismissals += 1;
                        cx.stop_propagation();
                    }
                }))
                .child(control)
                .child(div().id("other-focus").tab_index(0).size(px(20.)))
        }
    }

    fn open(cx: &mut TestAppContext, scheme: bool) -> (Entity<Harness>, VisualTestContext) {
        cx.update(crate::init);
        let window = cx.open_window(size(px(400.), px(400.)), |_, _| Harness {
            scheme,
            open: true,
            disabled: false,
            selected: 0,
            options: [
                "Alpha",
                "Alpine",
                "Beta",
                "Betamax",
                "One Dark",
                "One Light",
            ]
            .into_iter()
            .map(str::to_owned)
            .chain((0..20).map(|i| format!("Item {i}")))
            .chain(["Zulu".to_owned()])
            .collect(),
            scroll: ScrollHandle::new(),
            changes: Vec::new(),
            dialog_dismissals: 0,
        });
        let entity = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();
        (entity, cx)
    }

    fn keys(cx: &mut VisualTestContext, keys: &str) {
        for key in keys.split_whitespace() {
            cx.simulate_keystrokes(key);
            cx.simulate_event(gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse(key).unwrap(),
            });
            cx.run_until_parked();
        }
    }

    fn selected(entity: &Entity<Harness>, cx: &mut VisualTestContext) -> usize {
        entity.read_with(cx, |this, _| this.selected)
    }

    #[gpui::test]
    fn typing_refines_the_visible_label_in_both_dropdowns(cx: &mut TestAppContext) {
        for scheme in [false, true] {
            let (entity, mut cx) = open(cx, scheme);
            for (key, expected) in [("shift-b", 2), ("e", 2), ("t", 2), ("a", 2), ("m", 3)] {
                keys(&mut cx, key);
                assert_eq!(selected(&entity, &mut cx), expected);
            }
            assert!(entity.read_with(&cx, |this, _| this.open));
            // No match keeps the selection and does not emit another change.
            keys(&mut cx, "z");
            assert_eq!(
                entity.read_with(&cx, |this, _| this.changes.clone()),
                [2, 3]
            );
            // Shortcuts are not search text; arrows still move one row.
            keys(&mut cx, "ctrl-a");
            assert_eq!(selected(&entity, &mut cx), 3);
            keys(&mut cx, "up");
            assert_eq!(selected(&entity, &mut cx), 2);
            keys(&mut cx, "z");
            assert_eq!(selected(&entity, &mut cx), 26);
            assert!(entity.read_with(&cx, |this, _| this.scroll.offset().y < px(0.)));
        }
    }

    #[gpui::test]
    fn closing_blurring_and_waiting_start_a_new_prefix(cx: &mut TestAppContext) {
        for scheme in [false, true] {
            let (entity, mut cx) = open(cx, scheme);
            keys(&mut cx, "b enter");
            assert!(!entity.read_with(&cx, |this, _| this.open));
            keys(&mut cx, "a");
            assert_eq!(selected(&entity, &mut cx), 0);
            keys(&mut cx, "tab shift-tab b");
            assert_eq!(selected(&entity, &mut cx), 2);
            cx.executor().advance_clock(TYPEAHEAD_TIMEOUT);
            keys(&mut cx, "a");
            assert_eq!(selected(&entity, &mut cx), 0);
            keys(&mut cx, "a");
            assert_eq!(selected(&entity, &mut cx), 1);
        }
    }

    #[gpui::test]
    fn spaces_in_names_do_not_activate_the_trigger(cx: &mut TestAppContext) {
        for scheme in [false, true] {
            let (entity, mut cx) = open(cx, scheme);
            for key in ["o", "n", "e", "space", "l"] {
                keys(&mut cx, key);
            }
            assert_eq!(selected(&entity, &mut cx), 5);
            assert!(entity.read_with(&cx, |this, _| this.open));
            cx.executor().advance_clock(TYPEAHEAD_TIMEOUT);
            keys(&mut cx, "space");
            assert!(!entity.read_with(&cx, |this, _| this.open));
        }
    }

    #[gpui::test]
    fn a_disabled_scheme_ignores_typing(cx: &mut TestAppContext) {
        let (entity, mut cx) = open(cx, true);
        entity.update(&mut cx, |this, cx| {
            this.disabled = true;
            cx.notify();
        });
        cx.run_until_parked();
        keys(&mut cx, "b");
        assert_eq!(selected(&entity, &mut cx), 0);
    }

    /// The harness hands neither dropdown a bar, as most owners hand none, so
    /// each runs its own: the pointer on the list's right edge puts it up, and
    /// dragging it scrolls the list without the press reaching the row under
    /// the thumb.
    #[gpui::test]
    fn a_list_without_an_owner_bar_scrolls_by_its_own(cx: &mut TestAppContext) {
        for scheme in [false, true] {
            let (entity, mut cx) = open(cx, scheme);
            let list = entity.read_with(&cx, |this, _| this.scroll.bounds());
            // The thumb starts at the top of the list's right edge.
            let thumb = point(list.right() - px(3.), list.top() + px(8.));
            cx.simulate_mouse_move(thumb, None, Modifiers::none());
            cx.run_until_parked();
            cx.simulate_mouse_down(thumb, MouseButton::Left, Modifiers::none());
            for step in 1..=6 {
                let to = point(thumb.x, thumb.y + px(10. * step as f32));
                cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
            }
            let end = point(thumb.x, thumb.y + px(60.));
            cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
            cx.run_until_parked();
            entity.read_with(&cx, |this, _| {
                assert!(this.scroll.offset().y < px(0.), "the drag scrolled nothing");
                assert!(this.changes.is_empty(), "the press picked a row");
                assert!(this.open);
            });
        }
    }

    #[gpui::test]
    fn escape_closes_only_the_dropdown_before_reaching_the_dialog(cx: &mut TestAppContext) {
        for scheme in [false, true] {
            let (entity, mut cx) = open(cx, scheme);
            keys(&mut cx, "b escape");
            entity.read_with(&cx, |this, _| {
                assert!(!this.open);
                assert_eq!(this.selected, 2);
                assert_eq!(this.changes, [2]);
                assert_eq!(this.dialog_dismissals, 0);
            });
            keys(&mut cx, "escape");
            assert_eq!(entity.read_with(&cx, |this, _| this.dialog_dismissals), 1);
        }
    }

    #[gpui::test]
    fn escape_in_an_open_dropdown_precedes_a_host_key_binding(cx: &mut TestAppContext) {
        for scheme in [false, true] {
            let (entity, mut cx) = open(cx, scheme);
            // Register the host binding last, as the applications do. A raw
            // key listener cannot stop this action from dismissing the dialog.
            cx.update(|_, cx| {
                cx.bind_keys([KeyBinding::new(
                    "escape",
                    DismissDialog,
                    Some("SelectTestDialog"),
                )]);
            });
            keys(&mut cx, "escape");
            entity.read_with(&cx, |this, _| {
                assert!(!this.open);
                assert_eq!(this.dialog_dismissals, 0);
            });
            keys(&mut cx, "escape");
            assert_eq!(entity.read_with(&cx, |this, _| this.dialog_dismissals), 1);
        }
    }

    #[gpui::test]
    fn sibling_dropdowns_keep_their_searches_separate(cx: &mut TestAppContext) {
        struct Pair([String; 2]);
        impl Render for Pair {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div().size_full().children((0..2).map(|index| {
                    let entity = cx.entity();
                    Select::new(("picker", index))
                        .options(["Alpha", "Alpine", "Beta"])
                        .selected(Some(self.0[index].clone()))
                        .on_select(move |_, label, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.0[index] = label.to_owned();
                                cx.notify();
                            });
                        })
                }))
            }
        }
        cx.update(crate::init);
        let window = cx.open_window(size(px(400.), px(400.)), |_, _| {
            Pair(["Alpha".into(), "Alpha".into()])
        });
        let entity = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        keys(&mut cx, "tab b tab a l p i");
        assert_eq!(
            entity.read_with(&cx, |this, _| this.0.clone()),
            ["Beta".to_owned(), "Alpine".to_owned()]
        );
    }
}
