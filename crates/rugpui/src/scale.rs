//! How large the interface is drawn, and the handful of text sizes every widget
//! here draws at.
//!
//! The applications built on this crate offer an "interface size" setting, and
//! until now the only thing it reached was the text size set on the root of the
//! window: a text field inherits that, but a tree row, a menu, a tab, a button
//! and a dialog title each named a size of their own in pixels and ignored it.
//! Raising the setting made the fields grow beside labels that stayed where they
//! were.
//!
//! [`set_ui_scale`] is the one knob, a factor installed as a [`Global`] the way
//! the theme is, because the readers are leaves that know nothing of a settings
//! file. Every widget here reads its text size through [`TextSize`] and the
//! heights that have to hold that text through [`scaled`], so at the default
//! factor of `1.0` everything is drawn at exactly the pixels it was before the
//! factor existed — `13. * 1.0` is `13.` — and at any other factor the whole
//! interface grows together. An application converts its own fixed sizes the
//! same way: `.text_size(TextSize::Body.size(cx))` for one of the four steps,
//! `.text_size(scaled(10.5, cx))` for a size of its own.
//!
//! Scaled along with the text is everything that stands beside it as a symbol
//! or holds it: icons and the fixed boxes that hold nothing but an icon (a tab's
//! close button, a menu trigger), the box and check of a checkbox, a switch's
//! track and knob, a slider's knob and track, the disclosure arrows of a tree
//! and a collapsible section and the indent of a tree level, color swatches and
//! status dots, a progress bar, a spinner's default size, and the fixed widths
//! that hold text — a dropdown's default width, a menu's panel, a label column.
//! An icon drawn 16 px beside 26 px text reads as a speck, and a checkbox that
//! stays put while its label doubles stops looking like the label's checkbox.
//!
//! What is deliberately *not* scaled: borders, gaps, paddings, corner radii,
//! scroll bar thickness, dividers and the hit areas of resize handles. They are
//! the frame around the content rather than the content, and scaling them too
//! would only make a larger interface look like a zoomed screenshot of the small
//! one. Neither are the caps an overlay or a dialog takes from the window:
//! a dialog whose width an application scaled still stops at the window's edge.
//!
//! An application holds to the same rule for its own sizes: an icon it draws is
//! `rugpui_shell::icons::icon(path, scaled(16., cx), color)`, not `px(16.)`.

use gpui::{App, Global, Pixels, px};

/// The smallest factor [`set_ui_scale`] accepts.
///
/// Half size is already smaller than any text a desktop draws by default; past
/// it a slip of a settings slider would leave nothing legible to slide it back
/// with.
pub const MIN_UI_SCALE: f32 = 0.5;

/// The largest factor [`set_ui_scale`] accepts.
///
/// Three times is a 39 px body text, past which a dialog no longer fits a
/// laptop screen at all.
pub const MAX_UI_SCALE: f32 = 3.0;

/// The installed factor, as a gpui global.
#[derive(Debug, Clone, Copy)]
struct UiScale(f32);

impl Global for UiScale {}

/// Sets how large the interface is drawn, `1.0` being the size every widget
/// was designed at.
///
/// Clamped to [`MIN_UI_SCALE`]..=[`MAX_UI_SCALE`]; a value that is not a
/// number at all is taken as `1.0`. Every open window is redrawn, cached views
/// included, so the change shows at once.
///
/// An application with an "interface size" in points maps it onto a factor
/// against the size it treats as normal:
///
/// ```ignore
/// rugpui::set_ui_scale(settings.ui_font_size / DEFAULT_UI_FONT_SIZE, cx);
/// ```
pub fn set_ui_scale(scale: f32, cx: &mut App) {
    let scale = if scale.is_finite() {
        scale.clamp(MIN_UI_SCALE, MAX_UI_SCALE)
    } else {
        1.0
    };
    cx.set_global(UiScale(scale));
    cx.refresh_windows();
}

/// The installed factor, or `1.0` when nobody has set one.
pub fn ui_scale(cx: &App) -> f32 {
    cx.try_global::<UiScale>().map_or(1.0, |scale| scale.0)
}

/// `value` pixels at the installed factor.
///
/// For a size the four [`TextSize`] steps do not name — a status bar's 11.5,
/// a heading's 18 — and for a height that has to hold text: a row, a button,
/// a header band. Exactly `px(value)` at the default factor.
pub fn scaled(value: f32, cx: &App) -> Pixels {
    px(value * ui_scale(cx))
}

/// The four text sizes the widgets draw at.
///
/// Named by role rather than by pixel so that an application which converts its
/// own labels to these lines up with the widgets beside them at every factor.
/// At the default factor each is the pixel size in its doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextSize {
    /// 11 px: hints, shortcut labels, tooltips, compact buttons, status lines.
    Xs,
    /// 12 px: secondary text — a segmented control, a caption under a value.
    Sm,
    /// 13 px: the body — rows, labels, buttons, tabs, menus.
    Body,
    /// 14 px: a dialog's title, a dropdown's trigger.
    Title,
}

impl TextSize {
    /// The size at the default factor, in pixels.
    pub const fn base(self) -> f32 {
        match self {
            Self::Xs => 11.,
            Self::Sm => 12.,
            Self::Body => 13.,
            Self::Title => 14.,
        }
    }

    /// The size at the installed factor.
    pub fn size(self, cx: &App) -> Pixels {
        scaled(self.base(), cx)
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;

    /// The promise the whole module rests on: nobody who leaves the factor
    /// alone sees a single pixel move.
    #[gpui::test]
    fn the_default_factor_is_the_old_pixels(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert_eq!(ui_scale(cx), 1.0);
            assert_eq!(TextSize::Xs.size(cx), px(11.));
            assert_eq!(TextSize::Sm.size(cx), px(12.));
            assert_eq!(TextSize::Body.size(cx), px(13.));
            assert_eq!(TextSize::Title.size(cx), px(14.));
            assert_eq!(scaled(24., cx), px(24.));
        });
    }

    #[gpui::test]
    fn a_factor_scales_every_step_and_stays_in_range(cx: &mut TestAppContext) {
        cx.update(|cx| {
            set_ui_scale(1.5, cx);
            assert_eq!(TextSize::Body.size(cx), px(19.5));
            assert_eq!(scaled(24., cx), px(36.));

            set_ui_scale(100., cx);
            assert_eq!(ui_scale(cx), MAX_UI_SCALE);
            set_ui_scale(0., cx);
            assert_eq!(ui_scale(cx), MIN_UI_SCALE);
            set_ui_scale(f32::NAN, cx);
            assert_eq!(ui_scale(cx), 1.0);
        });
    }

    /// A checkbox, a switch and a menu trigger with its icon, side by side.
    struct Symbols;

    impl gpui::Render for Symbols {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{ParentElement, Styled};
            gpui::div()
                .flex()
                .flex_col()
                .size_full()
                .child(crate::Checkbox::new("check", "Check"))
                .child(crate::Switch::new("switch", "Switch"))
                .child(crate::MenuButton::new("menu"))
        }
    }

    /// The symbols beside the text grow with it, and at the default factor
    /// they are the pixels they always were.
    #[gpui::test]
    fn icons_and_toggles_grow_with_the_interface(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let window = cx.add_window(|_, _| Symbols);
        let mut cx = gpui::VisualTestContext::from_window(window.into(), cx);
        cx.simulate_resize(gpui::size(px(400.), px(300.)));
        let measure = |cx: &mut gpui::VisualTestContext, scale: f32| {
            cx.update(|_, cx| set_ui_scale(scale, cx));
            cx.run_until_parked();
            [
                "checkbox-box",
                "switch-track",
                "menu-trigger",
                "menu-trigger-icon",
            ]
            .map(|name| {
                cx.debug_bounds(name)
                    .unwrap_or_else(|| panic!("{name} is not drawn"))
                    .size
            })
        };

        let [check, switch, trigger, icon] = measure(&mut cx, 1.0);
        assert_eq!(check, gpui::size(px(16.), px(16.)));
        assert_eq!(switch, gpui::size(px(30.), px(16.)));
        assert_eq!(trigger, gpui::size(px(28.), px(28.)));
        assert_eq!(icon, gpui::size(px(16.), px(16.)));

        let [check, switch, trigger, icon] = measure(&mut cx, 1.5);
        assert_eq!(check, gpui::size(px(24.), px(24.)));
        assert_eq!(switch, gpui::size(px(45.), px(24.)));
        assert_eq!(trigger, gpui::size(px(42.), px(42.)));
        assert_eq!(icon, gpui::size(px(24.), px(24.)));
    }
}
