//! The window frame an application draws when the platform will not.
//!
//! A window is opened one of two ways — with the caption the operating system
//! draws, or with a transparent title bar and the application's own toolbar
//! standing in for it — and [`TitlebarStyle`] is the setting that says which.
//! Everything else here follows from the second choice, and it is far more than
//! a row of buttons: taking the caption away also takes the drag, the
//! double-click to maximise, the window menu, the resize borders and the drop
//! shadow, and each of them has to be put back by hand and differently on every
//! platform.
//!
//! The pieces:
//!
//! * [`draws_own_titlebar`] — whether the row *is* the caption right now, which
//!   on Linux is a question about the window and not only about the setting.
//! * [`titlebar_gestures`] — the drag, the double-click and the window menu,
//!   wired onto the row that stands in for the caption.
//! * [`client_tiling`] — whether the window carries the shadow band, and which
//!   of its edges currently touch something.
//! * [`render_resize_edges`] — the resize grips that band doubles as.
//! * [`window_appearance`] — blur, translucency or neither.
//! * [`window_control_strips`] — the caption buttons, split into the two ends a
//!   Linux desktop may ask for.
//!
//! Nothing here reads the host application's settings: every function takes the
//! two or three values it needs, so an application whose settings type is
//! shaped differently still calls the same code.

use gpui::{
    AnyElement, App, Div, MouseButton, Stateful, Window, WindowBackgroundAppearance, div,
    prelude::*, px,
};
use ruui::{WindowControlIcons, WindowControls, window_controls};
use serde::{Deserialize, Serialize};

/// Width of the transparent band around a self-decorated window.
///
/// The band carries the drop shadow the compositor no longer draws once the
/// window asks for client-side decorations, and doubles as the resize grip. It
/// is part of the window's surface but not of the window as the user
/// understands it: [`Window::set_client_inset`] publishes the visible bounds
/// through `_GTK_FRAME_EXTENTS`, so the compositor snaps, maximises and stacks
/// by the visible edge, exactly as it does for GTK's frames.
pub const SHADOW_BAND: f32 = 12.;

/// Edge length of the corner squares, where the resize goes diagonal.
pub const RESIZE_CORNER: f32 = 24.;

/// Who draws the window's title bar.
///
/// Read once, when the window is created: the platforms decide at that point
/// whether the window has a caption at all, so a change only shows after a
/// restart. The interface offering the setting is expected to say so.
///
/// Serialised in `snake_case`, which is the spelling the settings files of
/// every application using this crate already carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TitlebarStyle {
    /// The application draws it: the toolbar doubles as the title bar. The
    /// default.
    #[default]
    Custom,
    /// The operating system draws its own caption above the application's
    /// chrome.
    System,
}

/// Whether the title bar row has to stand in for the window's caption.
///
/// On Windows and macOS the style applied to the window settles it: a
/// transparent title bar leaves no platform caption, so the row is all there
/// is.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn draws_own_titlebar(style: TitlebarStyle, _window: &Window) -> bool {
    style == TitlebarStyle::Custom
}

/// Whether the title bar row has to stand in for the window's caption.
///
/// Linux is not the configured style alone. The custom style makes the window
/// ask for client-side decorations, but the ask can be declined — gpui falls
/// back to server decorations when no compositor is running — so what the
/// window actually ended up with is what decides here. Deciding from the style
/// alone would draw a second caption under the compositor's own.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn draws_own_titlebar(style: TitlebarStyle, window: &Window) -> bool {
    style == TitlebarStyle::Custom
        && matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        )
}

/// Wires the gestures a system title bar answers to onto the custom one.
///
/// Windows needs none of them. The row reports itself as
/// [`WindowControlArea::Drag`](gpui::WindowControlArea::Drag), the hit test
/// turns that into `HTCAPTION`, and the window procedure then does the
/// dragging, the aero-snap gestures and the double-click to maximise on its
/// own — before the application is ever told a button went down.
#[cfg(target_os = "windows")]
pub fn titlebar_gestures(row: Stateful<Div>) -> Stateful<Div> {
    row
}

/// Wires the gestures a system title bar answers to onto the custom one.
///
/// AppKit still drags the window for the strip its own title bar would have
/// covered, so only the double-click is left to answer — and it has to go
/// through [`Window::titlebar_double_click`], which follows whatever the user
/// picked in System Settings (zoom, minimise, or nothing at all).
#[cfg(target_os = "macos")]
pub fn titlebar_gestures(row: Stateful<Div>) -> Stateful<Div> {
    row.on_click(|event, window, _cx| {
        if event.standard_click() && event.click_count() == 2 {
            window.titlebar_double_click();
        }
    })
}

/// Wires the gestures a system title bar answers to onto the custom one.
///
/// Everything is the application's here: the compositor is told to take over
/// the move, and the window menu and the zoom have to be asked for explicitly.
/// Only meaningful once the window carries client-side decorations, which is
/// why the caller gates them on [`draws_own_titlebar`].
///
/// The move starts on the press rather than the click because the compositor
/// takes the pointer with it, so a release would never arrive.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn titlebar_gestures(row: Stateful<Div>) -> Stateful<Div> {
    row.on_click(|event, window, _cx| {
        if event.standard_click() && event.click_count() == 2 {
            window.zoom_window();
        }
    })
    .on_mouse_down(MouseButton::Left, |_, window, _cx| {
        window.start_window_move();
    })
    .on_mouse_down(MouseButton::Right, |event, window, _cx| {
        window.show_window_menu(event.position);
    })
}

/// The tiling state of a window that draws its own frame, `None` under a
/// server-side one.
///
/// Always `None` here: Windows keeps resizing and framing the window through
/// the caption hit test even under a custom title bar, and AppKit never gives
/// the frame up at all — neither window ever carries the shadow band.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub fn client_tiling(_window: &Window) -> Option<gpui::Tiling> {
    None
}

/// The tiling state of a window that draws its own frame, `None` under a
/// server-side one.
///
/// `Some` exactly when the compositor granted client-side decorations, with the
/// edges that currently touch a screen or neighbour edge marked tiled — those
/// edges get no band, no shadow and no resize grip. Fullscreen counts as tiled
/// all round.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn client_tiling(window: &Window) -> Option<gpui::Tiling> {
    match window.window_decorations() {
        gpui::Decorations::Client { tiling } => Some(tiling),
        gpui::Decorations::Server => None,
    }
}

/// The resize handles the compositor's frame would have provided.
///
/// Asking for client-side decorations takes the frame away, resize borders
/// included, so the shadow band has to start the resize itself — the compositor
/// takes over once told, exactly as it does for the title-bar drag. The strips
/// cover the band, the corner squares reach past it into the window, and every
/// tiled edge goes without: a maximised or snapped window has no border to drag
/// there.
pub fn render_resize_edges(tiling: gpui::Tiling) -> Vec<AnyElement> {
    use gpui::{CursorStyle, ResizeEdge};

    let strip = px(SHADOW_BAND);
    let corner = px(RESIZE_CORNER);
    // A strip stops short of a corner square only where that square exists;
    // against a tiled perpendicular edge it runs to the end of the band.
    let inset = |tiled: bool| if tiled { px(0.) } else { corner };
    let handle = |id: &'static str, cursor: CursorStyle, edge: ResizeEdge| {
        div()
            .id(id)
            .occlude()
            .absolute()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, _cx| {
                window.start_window_resize(edge);
            })
    };

    let mut handles: Vec<AnyElement> = Vec::new();
    if !tiling.top {
        handles.push(
            handle("resize-top", CursorStyle::ResizeUpDown, ResizeEdge::Top)
                .top_0()
                .left(inset(tiling.left))
                .right(inset(tiling.right))
                .h(strip)
                .into_any_element(),
        );
    }
    if !tiling.bottom {
        handles.push(
            handle(
                "resize-bottom",
                CursorStyle::ResizeUpDown,
                ResizeEdge::Bottom,
            )
            .bottom_0()
            .left(inset(tiling.left))
            .right(inset(tiling.right))
            .h(strip)
            .into_any_element(),
        );
    }
    if !tiling.left {
        handles.push(
            handle(
                "resize-left",
                CursorStyle::ResizeLeftRight,
                ResizeEdge::Left,
            )
            .left_0()
            .top(inset(tiling.top))
            .bottom(inset(tiling.bottom))
            .w(strip)
            .into_any_element(),
        );
    }
    if !tiling.right {
        handles.push(
            handle(
                "resize-right",
                CursorStyle::ResizeLeftRight,
                ResizeEdge::Right,
            )
            .right_0()
            .top(inset(tiling.top))
            .bottom(inset(tiling.bottom))
            .w(strip)
            .into_any_element(),
        );
    }
    if !tiling.top && !tiling.left {
        handles.push(
            handle(
                "resize-top-left",
                CursorStyle::ResizeUpLeftDownRight,
                ResizeEdge::TopLeft,
            )
            .top_0()
            .left_0()
            .size(corner)
            .into_any_element(),
        );
    }
    if !tiling.top && !tiling.right {
        handles.push(
            handle(
                "resize-top-right",
                CursorStyle::ResizeUpRightDownLeft,
                ResizeEdge::TopRight,
            )
            .top_0()
            .right_0()
            .size(corner)
            .into_any_element(),
        );
    }
    if !tiling.bottom && !tiling.left {
        handles.push(
            handle(
                "resize-bottom-left",
                CursorStyle::ResizeUpRightDownLeft,
                ResizeEdge::BottomLeft,
            )
            .bottom_0()
            .left_0()
            .size(corner)
            .into_any_element(),
        );
    }
    if !tiling.bottom && !tiling.right {
        handles.push(
            handle(
                "resize-bottom-right",
                CursorStyle::ResizeUpLeftDownRight,
                ResizeEdge::BottomRight,
            )
            .bottom_0()
            .right_0()
            .size(corner)
            .into_any_element(),
        );
    }
    handles
}

/// Maps the window settings onto a gpui background appearance.
///
/// Blur wins when requested; failing that, any opacity below fully opaque asks
/// for a plain transparent window; otherwise the window stays opaque.
///
/// The two values are taken apart rather than as one settings struct, because
/// the three applications spell that struct three different ways and neither of
/// them is this crate's to name.
pub fn window_appearance(
    background_blur: bool,
    background_opacity: f32,
) -> WindowBackgroundAppearance {
    if background_blur {
        WindowBackgroundAppearance::Blurred
    } else if background_opacity < 1.0 {
        WindowBackgroundAppearance::Transparent
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// The caption buttons of a self-drawn title bar, as the two strips a title bar
/// draws them in.
///
/// Two strips rather than one, because a Linux desktop decides where its
/// caption buttons go and putting them on the left is a setting people actually
/// use; [`window_controls::split`] turns what the platform reports into the two
/// ends. Off Linux nothing is reported, which is the same answer as "the usual
/// three on the right".
///
/// Both are `None` unless the row is standing in for the caption — pass what
/// [`draws_own_titlebar`] answered as `custom` — and both are `None` on macOS,
/// where AppKit goes on drawing the traffic lights over the application's own
/// toolbar band and a second set would be two ways to close one window.
///
/// The element ids are fixed, so a caller renders the pair straight into its
/// title bar row: `window-controls-leading` before whatever the row starts
/// with, `window-controls-trailing` after whatever it ends with.
pub fn window_control_strips(
    icons: &WindowControlIcons,
    custom: bool,
    window: &Window,
    cx: &App,
) -> (Option<WindowControls>, Option<WindowControls>) {
    let (leading, trailing) = if custom && !cfg!(target_os = "macos") {
        window_controls::split(cx.button_layout(), window.window_controls())
    } else {
        (Vec::new(), Vec::new())
    };
    let strip = |id: &'static str, buttons: Vec<gpui::WindowButton>| {
        (!buttons.is_empty()).then(|| WindowControls::new(id, icons.clone(), buttons))
    };
    (
        strip("window-controls-leading", leading),
        strip("window-controls-trailing", trailing),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blurred_window_is_blurred_whatever_its_opacity() {
        assert_eq!(
            window_appearance(true, 1.0),
            WindowBackgroundAppearance::Blurred
        );
        assert_eq!(
            window_appearance(true, 0.5),
            WindowBackgroundAppearance::Blurred
        );
    }

    #[test]
    fn anything_short_of_fully_opaque_asks_for_a_transparent_surface() {
        assert_eq!(
            window_appearance(false, 0.99),
            WindowBackgroundAppearance::Transparent
        );
        assert_eq!(
            window_appearance(false, 1.0),
            WindowBackgroundAppearance::Opaque
        );
    }

    #[test]
    fn the_titlebar_style_round_trips_through_the_spelling_the_settings_use() {
        let custom = serde_json::to_string(&TitlebarStyle::Custom).expect("a unit variant");
        let system = serde_json::to_string(&TitlebarStyle::System).expect("a unit variant");
        assert_eq!(custom, "\"custom\"");
        assert_eq!(system, "\"system\"");
        assert_eq!(
            serde_json::from_str::<TitlebarStyle>("\"system\"").expect("a known variant"),
            TitlebarStyle::System
        );
        assert_eq!(TitlebarStyle::default(), TitlebarStyle::Custom);
    }
}
