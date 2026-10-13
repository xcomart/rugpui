//! Rendering boundaries for panes whose parent already determines their size.

use gpui::{Entity, IntoElement, Render, StyleRefinement, relative};

/// Fill the parent and reuse the view until it or a dependency changes.
///
/// Use inside a pane with a definite size, not a content-sized form row. Child
/// notifications, focus changes, resizing and window refreshes still redraw it.
pub fn cached_view<V: Render>(view: Entity<V>) -> impl IntoElement {
    let mut style = StyleRefinement::default();
    style.size.width = Some(relative(1.).into());
    style.size.height = Some(relative(1.).into());
    view.cached(style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, ScrollHandle, TestAppContext, VisualTestContext, Window, div, prelude::*, px, size,
    };
    use std::{cell::Cell, rc::Rc};

    struct Counter(Rc<Cell<usize>>, ScrollHandle);
    impl Render for Counter {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.0.set(self.0.get() + 1);
            div().id("pane-content").size_full().track_scroll(&self.1)
        }
    }

    struct Harness {
        pane: Entity<Counter>,
    }
    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().h(px(40.)).flex_none())
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(cached_view(self.pane.clone())),
                )
                .child(div().h(px(30.)).flex_none())
        }
    }

    #[gpui::test]
    fn unrelated_frames_reuse_the_pane_but_content_and_refresh_redraw_it(cx: &mut TestAppContext) {
        let renders = Rc::new(Cell::new(0));
        let bounds = ScrollHandle::new();
        let pane = cx.new(|_| Counter(renders.clone(), bounds.clone()));
        let window = cx.add_window({
            let pane = pane.clone();
            move |_, _| Harness { pane }
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(size(px(300.), px(200.)));
        cx.run_until_parked();
        assert_eq!(bounds.bounds().size, size(px(300.), px(130.)));
        let initial = renders.get();
        assert!(initial > 0);
        for _ in 0..20 {
            window.update(&mut cx, |_, _, cx| cx.notify()).unwrap();
            cx.run_until_parked();
        }
        assert_eq!(
            renders.get(),
            initial,
            "overlay frames must reuse an unchanged pane"
        );
        pane.update(&mut cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(renders.get(), initial + 1);
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(
            renders.get(),
            initial + 2,
            "theme and settings refreshes must reach cached panes"
        );
        cx.simulate_resize(size(px(800.), px(600.)));
        cx.run_until_parked();
        assert_eq!(bounds.bounds().size, size(px(800.), px(530.)));
        assert!(renders.get() > initial + 2);
    }
}
