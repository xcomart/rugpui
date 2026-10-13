//! Smooth wheel scrolling for ordinary, vertically scrolling form surfaces.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, Div, Element, ElementId, EntityId, GlobalElementId,
    InspectorElementId, InteractiveElement, Interactivity, IntoElement, LayoutId, ParentElement,
    Pixels, ScrollHandle, Stateful, StatefulInteractiveElement, StyleRefinement, Styled, Window,
    px,
};

const DURATION: Duration = Duration::from_millis(120);

#[derive(Default)]
struct Motion {
    from: f32,
    target: f32,
    last: f32,
    started: Option<Instant>,
    pending: bool,
}

impl Motion {
    fn cancel(&mut self) {
        self.started = None;
    }

    fn queue(&mut self, current: f32, delta: f32, max: f32, now: Instant) -> bool {
        if self.started.is_some() && current != self.last {
            self.cancel();
        }
        let base = if self.started.is_some() && delta * (self.target - current) >= 0. {
            self.target
        } else {
            current
        };
        let target = (base + delta).clamp(-max.max(0.), 0.);
        if target == current {
            self.cancel();
            return false;
        }
        self.from = current;
        self.last = current;
        self.target = target;
        self.started = Some(now);
        true
    }

    fn step(&mut self, current: f32, max: f32, now: Instant, reduced: bool) -> f32 {
        let Some(started) = self.started else {
            return current;
        };
        // Thumb dragging, focus reveal and programmatic scrolling take over.
        if current != self.last {
            self.cancel();
            return current;
        }
        self.target = self.target.clamp(-max.max(0.), 0.);
        let progress = if reduced {
            1.
        } else {
            (now.saturating_duration_since(started).as_secs_f32() / DURATION.as_secs_f32()).min(1.)
        };
        let eased = 1. - (1. - progress).powi(3);
        let next = (self.from + (self.target - self.from) * eased).clamp(-max.max(0.), 0.);
        if progress >= 1. || (next - self.target).abs() < 0.1 {
            self.cancel();
            self.last = self.target;
        } else {
            self.last = next;
        }
        self.last
    }
}

fn next_frame(motion: Rc<RefCell<Motion>>, handle: ScrollHandle, view: EntityId, window: &Window) {
    if motion.borrow().pending {
        return;
    }
    motion.borrow_mut().pending = true;
    window.on_next_frame(move |window, cx| {
        let mut state = motion.borrow_mut();
        state.pending = false;
        let mut offset = handle.offset();
        let next = state.step(
            f32::from(offset.y),
            f32::from(handle.max_offset().y),
            cx.background_executor().now(),
            cx.reduce_motion(),
        );
        if offset.y != px(next) {
            offset.y = px(next);
            handle.set_offset(offset);
            cx.notify(view);
        }
        let active = state.started.is_some();
        drop(state);
        if active {
            next_frame(motion, handle, view, window);
        }
    });
}

/// Add wheel interpolation to a div already using `overflow_y_scroll`.
pub trait SmoothScrollExt {
    /// Use the same handle passed to `track_scroll`. Precise input is left to
    /// the platform; reduced motion disables interpolation.
    fn smooth_scroll(self, handle: &ScrollHandle) -> SmoothScroll;
}

impl SmoothScrollExt for Stateful<Div> {
    fn smooth_scroll(self, handle: &ScrollHandle) -> SmoothScroll {
        let id = ElementId::from((Element::id(&self).expect("stateful div"), "smooth-scroll"));
        SmoothScroll {
            id,
            inner: Some(self),
            handle: handle.clone(),
        }
    }
}

/// A scrolling div with frame-driven wheel motion and persistent element state.
pub struct SmoothScroll {
    id: ElementId,
    inner: Option<Stateful<Div>>,
    handle: ScrollHandle,
}

impl Styled for SmoothScroll {
    fn style(&mut self) -> &mut StyleRefinement {
        self.inner.as_mut().unwrap().style()
    }
}

impl ParentElement for SmoothScroll {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.as_mut().unwrap().extend(elements);
    }
}

impl InteractiveElement for SmoothScroll {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.inner.as_mut().unwrap().interactivity()
    }
}

impl StatefulInteractiveElement for SmoothScroll {}

impl IntoElement for SmoothScroll {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for SmoothScroll {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let motion =
            window.with_element_state(id.unwrap(), |state: Option<Rc<RefCell<Motion>>>, _| {
                let state = state.unwrap_or_default();
                (state.clone(), state)
            });
        let handle = self.handle.clone();
        let view = window.current_view();
        let line_height = window.with_text_style(
            Some(self.inner.as_mut().unwrap().style().text.clone()),
            |window| window.line_height(),
        );
        let mut inner = self
            .inner
            .take()
            .unwrap()
            .on_scroll_wheel(move |event, window, cx| {
                if event.delta.precise() || cx.reduce_motion() {
                    motion.borrow_mut().cancel();
                    return;
                }
                let delta = event.delta.pixel_delta(line_height);
                if delta.y == px(0.) || delta.x != px(0.) {
                    return;
                }
                if motion.borrow_mut().queue(
                    f32::from(handle.offset().y),
                    f32::from(delta.y),
                    f32::from(handle.max_offset().y),
                    cx.background_executor().now(),
                ) {
                    next_frame(motion.clone(), handle.clone(), view, window);
                    cx.stop_propagation();
                }
            })
            .into_any_element();
        (inner.request_layout(window, cx), inner)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        inner: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        inner.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        inner: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        inner.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, Render, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, div,
        point, size,
    };

    struct Harness {
        scroll: ScrollHandle,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("scroll")
                .size_full()
                .line_height(px(30.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .smooth_scroll(&self.scroll)
                .child(div().h(px(1000.)).w_full())
        }
    }

    #[gpui::test]
    fn wheel_is_interpolated_once_and_precise_input_cancels_it(cx: &mut TestAppContext) {
        let scroll = ScrollHandle::new();
        let window = cx.add_window({
            let scroll = scroll.clone();
            move |_, _| Harness { scroll }
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(size(px(300.), px(200.)));
        cx.run_until_parked();
        let wheel = ScrollWheelEvent {
            position: point(px(50.), px(50.)),
            delta: ScrollDelta::Lines(point(0., -3.)),
            ..Default::default()
        };
        cx.simulate_event(wheel);
        assert_eq!(
            scroll.offset().y,
            px(0.),
            "native scrolling must not apply the delta first"
        );
        cx.executor().advance_clock(DURATION / 2);
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        cx.run_until_parked();
        let halfway = scroll.offset().y;
        assert!(halfway < px(0.) && halfway > px(-90.));
        cx.executor().advance_clock(DURATION / 2);
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            scroll.offset().y,
            px(-90.),
            "use the surface's own line height"
        );
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(50.)),
            delta: ScrollDelta::Lines(point(0., -3.)),
            ..Default::default()
        });
        let halfway = scroll.offset().y;

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(50.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-7.))),
            ..Default::default()
        });
        assert_eq!(scroll.offset().y, halfway - px(7.));
        cx.executor().advance_clock(DURATION);
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        cx.run_until_parked();
        assert_eq!(
            scroll.offset().y,
            halfway - px(7.),
            "pending wheel motion must not override the touchpad"
        );
    }

    #[gpui::test]
    fn wheel_respects_reduced_motion_and_programmatic_reveal(cx: &mut TestAppContext) {
        let scroll = ScrollHandle::new();
        let window = cx.add_window({
            let scroll = scroll.clone();
            move |_, _| Harness { scroll }
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(size(px(300.), px(200.)));
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.run_until_parked();
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(50.)),
            delta: ScrollDelta::Lines(point(0., -3.)),
            ..Default::default()
        });
        assert!(
            scroll.offset().y < px(0.),
            "reduced motion scrolls immediately"
        );
        cx.update(|_, cx| cx.set_reduce_motion(false));
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(50.)),
            delta: ScrollDelta::Lines(point(0., -3.)),
            ..Default::default()
        });
        scroll.set_offset(point(px(0.), px(-400.)));
        cx.executor().advance_clock(DURATION);
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
        });
        cx.run_until_parked();
        assert_eq!(scroll.offset().y, px(-400.));
    }

    #[test]
    fn wheel_motion_accumulates_and_finishes_at_the_requested_position() {
        let now = Instant::now();
        let mut motion = Motion::default();
        assert!(motion.queue(0., -60., 500., now));
        let half = motion.step(0., 500., now + DURATION / 2, false);
        assert!(half < 0. && half > -60.);
        assert!(motion.queue(half, -60., 500., now + DURATION / 2));
        assert_eq!(motion.step(half, 500., now + DURATION * 2, false), -120.);
        assert!(motion.started.is_none());
    }

    #[test]
    fn dragging_or_revealing_a_field_cancels_pending_motion() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.queue(0., -60., 500., now);
        assert_eq!(motion.step(-200., 500., now + DURATION, false), -200.);
        assert!(motion.started.is_none());
    }

    #[test]
    fn reverse_input_changes_direction_immediately_and_stays_in_bounds() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.queue(-100., -500., 200., now);
        let current = motion.step(-100., 200., now + DURATION / 2, false);
        motion.queue(current, 40., 200., now + DURATION / 2);
        assert!(motion.step(current, 200., now + DURATION, false) > current);
        let current = motion.last;
        motion.queue(current, -500., 200., now + DURATION);
        assert_eq!(motion.step(current, 50., now + DURATION * 2, false), -50.);
    }

    #[test]
    fn reduced_motion_finishes_without_extra_frames() {
        let now = Instant::now();
        let mut motion = Motion::default();
        motion.queue(0., -60., 500., now);
        assert_eq!(motion.step(0., 500., now, true), -60.);
        assert!(motion.started.is_none());
    }
}
