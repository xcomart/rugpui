//! The keys behind [`GridRequest`]: what they ask once a host opts in, and
//! that they reach whatever they reached before when it has not.

use gpui::actions;

use super::*;

actions!(grid_request_test, [HostDelete, HostEdit, HostPaste]);

/// Key context of the view around the grid, which binds the same three keys
/// the grid does — the way a host that had a `Delete` command of its own
/// before the grid knew about the key would.
const HOST_CONTEXT: &str = "RequestHost";

/// A view holding a grid and binding the request keys itself, one context
/// further out.
struct Host<S: GridSource> {
    grid: Entity<GridView<S>>,
    /// The host's own bindings, in the order they fired.
    actions: Rc<RefCell<Vec<&'static str>>>,
    /// Every key that reached the host's key-down listener.
    keys: Rc<RefCell<Vec<String>>>,
}

impl<S: GridSource> Render for Host<S> {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let (delete, edit, paste) = (
            self.actions.clone(),
            self.actions.clone(),
            self.actions.clone(),
        );
        let keys = self.keys.clone();
        div()
            .key_context(HOST_CONTEXT)
            .size_full()
            .on_action(move |_: &HostDelete, _, _| delete.borrow_mut().push("delete"))
            .on_action(move |_: &HostEdit, _, _| edit.borrow_mut().push("edit"))
            .on_action(move |_: &HostPaste, _, _| paste.borrow_mut().push("paste"))
            .on_key_down(move |event: &KeyDownEvent, _, _| {
                keys.borrow_mut().push(event.keystroke.key.clone());
            })
            .child(self.grid.clone())
    }
}

/// What a test reads back.
struct Seen<S: GridSource> {
    grid: Entity<GridView<S>>,
    requests: Rc<RefCell<Vec<GridRequest>>>,
    actions: Rc<RefCell<Vec<&'static str>>>,
    keys: Rc<RefCell<Vec<String>>>,
}

impl<S: GridSource> Seen<S> {
    fn requests(&self) -> Vec<GridRequest> {
        self.requests.borrow_mut().drain(..).collect()
    }

    fn actions(&self) -> Vec<&'static str> {
        self.actions.borrow_mut().drain(..).collect()
    }

    fn keys(&self) -> Vec<String> {
        self.keys.borrow_mut().drain(..).collect()
    }
}

/// The paste chord on the platform the test runs on.
fn paste_chord() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

/// Opens a focused grid over `source`, inside a host that binds the request
/// keys, with the requests on or off.
fn open_host<S: GridSource>(
    source: S,
    requests: bool,
    cx: &mut TestAppContext,
) -> (Seen<S>, VisualTestContext) {
    cx.update(|cx| {
        rugpui::init(cx);
        crate::init(cx);
        cx.bind_keys([
            KeyBinding::new("delete", HostDelete, Some(HOST_CONTEXT)),
            KeyBinding::new("f2", HostEdit, Some(HOST_CONTEXT)),
            KeyBinding::new(paste_chord(), HostPaste, Some(HOST_CONTEXT)),
        ]);
    });

    let asked: Rc<RefCell<Vec<GridRequest>>> = Rc::default();
    let actions: Rc<RefCell<Vec<&'static str>>> = Rc::default();
    let keys: Rc<RefCell<Vec<String>>> = Rc::default();
    let window = cx.add_window({
        let (asked, actions, keys) = (asked.clone(), actions.clone(), keys.clone());
        move |_, cx| {
            let grid = cx.new(|cx| {
                GridView::new(source, cx)
                    .fixed_widths()
                    .key_requests(requests)
            });
            cx.subscribe(
                &grid,
                move |_: &mut Host<S>, _, request: &GridRequest, _| {
                    asked.borrow_mut().push(request.clone());
                },
            )
            .detach();
            Host {
                grid,
                actions,
                keys,
            }
        }
    });
    let grid = window
        .update(cx, |host, _, _| host.grid.clone())
        .expect("the window is open");
    let mut cx = VisualTestContext::from_window(*window.deref(), cx);
    cx.update(|window, cx| grid.read(cx).focus_handle(cx).focus(window, cx));
    cx.run_until_parked();
    (
        Seen {
            grid,
            requests: asked,
            actions,
            keys,
        },
        cx,
    )
}

/// Puts `text` on the clipboard.
fn clip(cx: &mut VisualTestContext, text: &str) {
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned())));
}

/// Picks the cell at `row` and display `column`.
fn pick<S: GridSource>(seen: &Seen<S>, cx: &mut VisualTestContext, row: usize, column: usize) {
    cx.update(|_, cx| {
        seen.grid
            .update(cx, |grid, cx| grid.select_cell(row, column, cx))
    });
    cx.run_until_parked();
}

/// A grid whose host has not opted in asks for nothing, and every one of the
/// keys reaches the host's own bindings and listeners exactly as it did
/// before the grid bound them.
#[gpui::test]
fn without_the_opt_in_every_key_passes_through(cx: &mut TestAppContext) {
    let (seen, mut cx) = open_host(Huge::new(10, 3, Rc::default()).editable(), false, cx);
    pick(&seen, &mut cx, 1, 1);
    clip(&mut cx, "pasted");

    cx.simulate_keystrokes(&format!("delete f2 {} a", paste_chord()));
    assert_eq!(seen.requests(), Vec::new());
    assert_eq!(seen.actions(), vec!["delete", "edit", "paste"]);
    assert_eq!(seen.keys(), vec!["a".to_owned()]);
    assert_eq!(
        cx.update(|_, cx| seen.grid.read(cx).editing()),
        None,
        "and nothing was opened"
    );
}

/// Opted in, each key with something to act on becomes a request and stops
/// at the grid.
#[gpui::test]
fn opted_in_each_key_asks_the_host(cx: &mut TestAppContext) {
    let (seen, mut cx) = open_host(Huge::new(10, 3, Rc::default()).editable(), true, cx);
    pick(&seen, &mut cx, 2, 1);

    cx.simulate_keystrokes("delete");
    assert_eq!(seen.requests(), vec![GridRequest::Delete]);

    cx.simulate_keystrokes("f2");
    assert_eq!(
        seen.requests(),
        vec![GridRequest::Edit {
            row: 2,
            column: 1,
            text: None
        }]
    );

    clip(&mut cx, "a\tb\nc\td");
    cx.simulate_keystrokes(paste_chord());
    assert_eq!(
        seen.requests(),
        vec![GridRequest::Paste {
            text: "a\tb\nc\td".into()
        }]
    );

    // A capital is typed with Shift, which is allowed; the character is the
    // one the keyboard produced.
    cx.simulate_keystrokes("x shift-y space");
    assert_eq!(
        seen.requests(),
        ["x", "Y", " "]
            .into_iter()
            .map(|text| GridRequest::Edit {
                row: 2,
                column: 1,
                text: Some(text.into()),
            })
            .collect::<Vec<_>>()
    );

    assert_eq!(
        seen.actions(),
        Vec::<&str>::new(),
        "the host heard none of them"
    );
    assert_eq!(seen.keys(), Vec::<String>::new());
}

/// Opted in, a key with nothing to act on still passes through: no
/// selection, a cell that takes no edit, an empty clipboard, a chord.
#[gpui::test]
fn opted_in_a_key_with_nothing_to_act_on_passes_through(cx: &mut TestAppContext) {
    let (seen, mut cx) = open_host(Huge::new(10, 3, Rc::default()), true, cx);

    // Nothing selected yet.
    clip(&mut cx, "pasted");
    cx.simulate_keystrokes(&format!("delete {}", paste_chord()));
    assert_eq!(seen.actions(), vec!["delete", "paste"]);

    // A cell under the cursor, but one the source will not let be edited.
    pick(&seen, &mut cx, 0, 0);
    cx.simulate_keystrokes("f2 q");
    assert_eq!(seen.actions(), vec!["edit"]);
    assert_eq!(seen.keys(), vec!["q".to_owned()]);

    // An empty clipboard has nothing to paste.
    clip(&mut cx, "");
    cx.simulate_keystrokes(paste_chord());
    assert_eq!(seen.actions(), vec!["paste"]);

    // A command chord is never typing.
    cx.simulate_keystrokes("ctrl-q alt-w");
    assert_eq!(seen.requests(), Vec::new());
}

/// While the inline editor is open the keys are the field's: typing goes
/// into it and nothing is asked.
#[gpui::test]
fn nothing_is_asked_while_a_field_is_open(cx: &mut TestAppContext) {
    let (seen, mut cx) = open_host(Huge::new(10, 3, Rc::default()).editable(), true, cx);
    cx.update(|window, cx| {
        seen.grid
            .update(cx, |grid, cx| assert!(grid.begin_edit(1, 1, window, cx)))
    });
    cx.run_until_parked();

    cx.simulate_input("ab");
    cx.simulate_keystrokes("delete");
    assert_eq!(seen.requests(), Vec::new());
    let typed = cx.update(|_, cx| {
        seen.grid
            .read(cx)
            .editor()
            .map(|input| input.read(cx).content().to_string())
    });
    assert_eq!(typed.as_deref(), Some("valueab"));
}

/// `begin_edit_with` opens the field holding what was typed, caret after it,
/// and stages it against the cell's own value.
#[gpui::test]
fn begin_edit_with_seeds_the_field_and_stages_the_typing(cx: &mut TestAppContext) {
    let (grid, mut cx) = open(Staged::new(), cx);
    grid.update_in(&mut cx, |grid, window, cx| {
        assert!(grid.begin_edit_with(1, 2, "x", window, cx));
    });
    assert_eq!(grid.typed(&mut cx).as_deref(), Some("x"));

    // The caret is after the seed, so typing carries on from it.
    cx.simulate_input("yz");
    assert_eq!(grid.typed(&mut cx).as_deref(), Some("xyz"));
    cx.simulate_keystrokes("enter");
    assert_eq!(
        grid.drain(),
        vec![GridEvent::EditCommitted {
            row: 1,
            column: 2,
            value: EditValue::Text("xyz".into()),
        }]
    );

    // Compared against the cell, not against the seed: typing the value the
    // cell already held stages nothing.
    grid.update_in(&mut cx, |grid, window, cx| {
        assert!(grid.begin_edit_with(0, 2, "here", window, cx));
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(grid.drain(), Vec::new());

    // And it refuses exactly where `begin_edit` does.
    grid.update_in(&mut cx, |grid, window, cx| {
        assert!(!grid.begin_edit_with(0, 0, "x", window, cx));
    });
}
