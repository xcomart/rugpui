//! Sizing shared by the elements drawn over the window: menus, dropdown lists
//! and dialogs.

use gpui::{Pixels, Window, px};

/// The tallest an overlay may draw in `window` with `margin` kept clear above
/// and below it.
///
/// Measured against [`Window::client_content_bounds`], not the viewport: under
/// client-side decorations the viewport also holds the transparent shadow band
/// on each side, and the content is clipped at its inner edge, so an overlay
/// sized by the viewport loses its top and bottom once the window is short.
pub(crate) fn max_height(window: &Window, margin: Pixels) -> Pixels {
    (window.client_content_bounds().size.height - margin * 2.).max(px(0.))
}

/// The widest an overlay may draw in `window` with `margin` kept clear to its
/// left and right — [`max_height`] across instead of down.
pub(crate) fn max_width(window: &Window, margin: Pixels) -> Pixels {
    (window.client_content_bounds().size.width - margin * 2.).max(px(0.))
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use gpui::{Context, IntoElement, Render, TestAppContext, VisualTestContext, div, point, size};

    use super::*;

    struct Empty;

    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// The shadow band comes off both edges along with the margin, and a
    /// window too short for the margins leaves nothing rather than less.
    #[gpui::test]
    fn the_shadow_band_comes_off_both_edges(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(320.), px(200.)), |_, _| Empty);
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.update(|window, _| {
            assert_eq!(max_height(window, px(6.)), px(188.));
            window.set_client_inset(px(36.));
            assert_eq!(
                window.client_content_bounds().origin,
                point(px(36.), px(36.))
            );
            assert_eq!(max_height(window, px(6.)), px(116.));
            assert_eq!(max_height(window, px(80.)), px(0.));
            assert_eq!(max_width(window, px(6.)), px(236.));
            assert_eq!(max_width(window, px(200.)), px(0.));
        });
    }
}
