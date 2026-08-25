//! The parts of an application's settings that are about the *window* rather
//! than about the application.
//!
//! Three of them:
//!
//! * [`WindowGeometry`] — where the window is and how big it is, in the shape a
//!   settings file records, with [`window_geometry`] reading it off a live
//!   window and [`window_bounds`] turning a saved one back into a placement.
//! * [`monospace_family`] — the fixed-pitch family to draw code with when the
//!   user has named none, which on two of the three platforms means asking what
//!   is installed.
//! * [`window_tint`] — the configured opacity applied to a background fill.
//!
//! The settings *type* stays in the application: the three that share this
//! shell spell theirs three different ways, and none of them is this crate's to
//! name. What is here takes the two or three values it needs and hands an
//! answer back.

use std::sync::OnceLock;

use gpui::{App, Bounds, Hsla, Pixels, Point, SharedString, Size, Window, WindowBounds, px, size};

/// fontconfig's generic alias for a fixed-pitch face.
///
/// Only Linux resolves it. It is the last answer [`monospace_family`] gives,
/// and the only one it gives there.
const GENERIC_MONOSPACE: &str = "monospace";

/// Fixed-pitch families to look for on Windows, best first.
///
/// Cascadia Mono ships with Windows 11 and with the Terminal on 10; Cascadia
/// Code is the same face with programming ligatures and stands in when only the
/// Terminal's own install is present. Consolas has been in Windows since Vista
/// and Courier New since far earlier, so between them the list cannot come up
/// empty on a real machine.
#[cfg(target_os = "windows")]
pub const MONOSPACE_CANDIDATES: &[&str] =
    &["Cascadia Mono", "Cascadia Code", "Consolas", "Courier New"];

/// Fixed-pitch families to look for on macOS, best first.
///
/// SF Mono arrives with the Terminal and with Xcode and is what the system's
/// own developer tools draw code in; Menlo has shipped since 10.6 and Monaco
/// since long before that.
#[cfg(target_os = "macos")]
pub const MONOSPACE_CANDIDATES: &[&str] = &["SF Mono", "Menlo", "Monaco"];

/// No candidates anywhere else: see [`monospace_family`].
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const MONOSPACE_CANDIDATES: &[&str] = &[];

/// The family to draw fixed-pitch text with when no editor font is configured.
///
/// The naive answer — the literal `"monospace"` — is a *fontconfig* alias, so
/// it resolves to a real fixed-pitch face on Linux and nowhere else. Windows
/// DirectWrite has no such family: gpui logs `monospace not found` and falls
/// back to the system UI font, which is proportional, so code and tabulated
/// output lose their columns. CoreText has no alias either. So on those two
/// platforms a family that actually exists has to be named, and the only way to
/// know which ones exist is to ask.
///
/// Off the two platforms that need it — Linux, and gpui's headless test
/// platform, whose font list is the fallback stack and nothing else — the
/// candidate list is empty or matches nothing and the alias is returned
/// unchanged.
///
/// Resolved once per process and cached: enumerating every installed family is
/// a platform call far too heavy for a render pass. A font installed while the
/// application is running is therefore not picked up until the next start,
/// which is a trade made knowingly — the alternative is paying for the
/// enumeration on every frame that draws a line of code.
///
/// This is the *fallback*, never the user's choice: an application asks for it
/// only when its own `editor_font_family` setting is `None`.
pub fn monospace_family(cx: &App) -> SharedString {
    static RESOLVED: OnceLock<SharedString> = OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            pick(MONOSPACE_CANDIDATES, &cx.text_system().all_font_names())
                .map_or_else(|| SharedString::new_static(GENERIC_MONOSPACE), Into::into)
        })
        .clone()
}

/// The first of `candidates` that `installed` offers, spelled as `installed`
/// spells it.
///
/// Compared without ASCII case, and the *installed* spelling is what comes
/// back: the platforms report families in their own casing (and DirectWrite in
/// the system locale), and the name handed to the text system afterwards should
/// be one it has already said it has. Order is the candidate list's, not the
/// installed list's — this answers "the best face that is here", not "the first
/// face alphabetically".
pub fn pick(candidates: &[&str], installed: &[String]) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        installed
            .iter()
            .find(|name| name.eq_ignore_ascii_case(candidate))
            .cloned()
    })
}

/// Where a window is and how big it is, as a settings file records it.
///
/// A type of its own rather than a whole window-settings struct, because only
/// these five values follow a live window. The opacity, the blur and the title
/// bar style that sit beside them in an application's settings are the user's
/// choices and are never written back from the window they were applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeometry {
    /// Left edge in screen coordinates.
    pub x: i32,
    /// Top edge in screen coordinates.
    pub y: i32,
    /// Width in logical pixels.
    pub width: u32,
    /// Height in logical pixels.
    pub height: u32,
    /// Whether the window was maximized. The bounds above are then the size it
    /// un-maximizes back to, which is what gpui hands out and what has to be
    /// restored alongside the maximized state.
    pub maximized: bool,
}

impl WindowGeometry {
    /// Reads a window's placement, rounded to whole logical pixels.
    ///
    /// A settings file is hand-editable, and a fractional window position is
    /// noise in it; a compositor that reports halves would otherwise write
    /// `1439.5` into a file a user is expected to read.
    pub fn of(bounds: Bounds<Pixels>, maximized: bool) -> Self {
        let value = |pixels: Pixels| f32::from(pixels).round();
        Self {
            x: value(bounds.origin.x) as i32,
            y: value(bounds.origin.y) as i32,
            width: value(bounds.size.width).max(0.) as u32,
            height: value(bounds.size.height).max(0.) as u32,
            maximized,
        }
    }

    /// The saved placement, or `None` when the settings carry no position.
    ///
    /// `None` is a first run, or a window that was never moved: the platform
    /// picks the placement then, and [`window_bounds`] centres the saved *size*
    /// on the active display rather than guessing at coordinates.
    pub fn saved(
        x: Option<i32>,
        y: Option<i32>,
        width: u32,
        height: u32,
        maximized: bool,
    ) -> Option<Self> {
        Some(Self {
            x: x?,
            y: y?,
            width,
            height,
            maximized,
        })
    }

    /// The placement as gpui bounds.
    pub fn bounds(&self) -> Bounds<Pixels> {
        Bounds {
            origin: Point {
                x: px(self.x as f32),
                y: px(self.y as f32),
            },
            size: Size {
                width: px(self.width as f32),
                height: px(self.height as f32),
            },
        }
    }
}

/// Where a live window is now, in the shape the settings record.
///
/// Fullscreen is reported as *not* maximized, and with the restore bounds
/// either way, so the size survives: coming back fullscreen with no title bar
/// and no way to tell why would read as a broken window.
///
/// The caller decides what to do with the answer — an application records it in
/// its own settings global and writes the file when the last window closes,
/// which is what keeps a file write out of the middle of a resize drag.
pub fn window_geometry(window: &Window) -> WindowGeometry {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Fullscreen(bounds) => (bounds, false),
    };
    WindowGeometry::of(bounds, maximized)
}

/// The placement to open a window at.
///
/// A saved position is used as it stands; without one the saved *size* is
/// centred on the active display, which is what a first run does and what a
/// window that has never been moved deserves.
pub fn window_bounds(
    saved: Option<WindowGeometry>,
    width: u32,
    height: u32,
    maximized: bool,
    cx: &mut App,
) -> WindowBounds {
    let bounds = match saved {
        Some(geometry) => geometry.bounds(),
        None => Bounds::centered(None, size(px(width as f32), px(height as f32)), cx),
    };
    if maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

/// Applies the configured window opacity to a background fill.
///
/// **At most one such fill may cover any given pixel**, and between them they
/// must leave no pixel of the body uncovered. The window surface starts out
/// fully transparent, so a single translucent fill lets the desktop (or the
/// acrylic blur behind the window) show through. A second one on top does not:
/// gpui's Windows renderer blends the alpha channel additively
/// (`SrcBlendAlpha = ONE, DestBlendAlpha = ONE`), so two fills of, say, 0.75
/// and 0.62 saturate the surface alpha at 1.0 and the window goes opaque. That
/// is why a toolbar and a status bar paint their surface untinted.
///
/// What sits *over* a tinted fill would each be a second fill on the same
/// pixels, so while the window is translucent a result grid or a canvas paints
/// no background at all: it asks [`ruui::window_translucent`] and skips it,
/// leaving the fill below as the only tinted one. Tinting them instead of
/// skipping is the trap this whole comment is about.
///
/// The opacity itself lives in a widget-layer global, so that the leaves which
/// have to agree with this can reach it; the application pushes it there with
/// [`ruui::set_window_tint`] at start-up and on a settings *save*. Which means
/// this follows the saved settings and in particular does not follow a preview
/// — deliberately. The fill is only half of what makes a window translucent:
/// the other half is the platform surface being told to permit alpha, which
/// happens in [`gpui::Window::set_background_appearance`] and only when the
/// settings are saved. Tinting ahead of that would compose against an opaque
/// surface and merely darken the window, which is a worse answer than not
/// previewing at all.
pub fn window_tint(color: Hsla, cx: &App) -> Hsla {
    // Deferred to the widget layer, which is where the leaves that have to
    // agree with this can reach it.
    ruui::window_tint(color, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placement_round_trips_through_whole_pixels() {
        let bounds = Bounds {
            origin: Point {
                x: px(1439.5),
                y: px(-12.4),
            },
            size: size(px(1280.6), px(800.)),
        };
        let geometry = WindowGeometry::of(bounds, true);

        assert_eq!(geometry.x, 1440);
        assert_eq!(geometry.y, -12);
        assert_eq!(geometry.width, 1281);
        assert_eq!(geometry.height, 800);
        assert!(geometry.maximized);

        let back = geometry.bounds();
        assert_eq!(f32::from(back.origin.x), 1440.);
        assert_eq!(f32::from(back.size.width), 1281.);
    }

    #[test]
    fn a_negative_size_can_never_be_recorded() {
        // No compositor should report one, and an unsigned field cannot hold
        // it: the clamp is what keeps the cast from wrapping to four billion.
        let bounds = Bounds {
            origin: Point {
                x: px(0.),
                y: px(0.),
            },
            size: size(px(-10.), px(-1.)),
        };
        let geometry = WindowGeometry::of(bounds, false);
        assert_eq!(geometry.width, 0);
        assert_eq!(geometry.height, 0);
    }

    #[test]
    fn a_window_that_was_never_moved_has_no_saved_placement() {
        assert_eq!(WindowGeometry::saved(None, None, 1280, 800, false), None);
        assert_eq!(WindowGeometry::saved(Some(4), None, 1280, 800, false), None);
        assert_eq!(WindowGeometry::saved(None, Some(4), 1280, 800, false), None);
        assert_eq!(
            WindowGeometry::saved(Some(4), Some(9), 1280, 800, true),
            Some(WindowGeometry {
                x: 4,
                y: 9,
                width: 1280,
                height: 800,
                maximized: true,
            })
        );
    }

    #[test]
    fn the_best_installed_face_wins_and_keeps_the_platforms_own_spelling() {
        let installed = vec![
            "courier new".to_string(),
            "CONSOLAS".to_string(),
            "Arial".to_string(),
        ];
        // Candidate order, not installed order.
        assert_eq!(
            pick(&["Cascadia Mono", "Consolas", "Courier New"], &installed),
            Some("CONSOLAS".to_string())
        );
        assert_eq!(
            pick(&["Courier New"], &installed),
            Some("courier new".to_string())
        );
        assert_eq!(pick(&["Menlo"], &installed), None);
        assert_eq!(pick(&[], &installed), None);
    }
}
