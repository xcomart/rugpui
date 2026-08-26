//! The two disclosure marks the widgets here draw, embedded in the binary.
//!
//! gpui's [`svg`](gpui::svg) element resolves its `path` through the
//! [`AssetSource`](gpui::AssetSource) the application was built with, and this
//! crate has no way to reach around that: an icon it wants drawn has to be one
//! the *host's* asset source answers for. So the bytes live here, in
//! [`ICONS`], and the host chains that table into whatever it installs:
//!
//! ```ignore
//! const OWN: &[(&str, &[u8])] = &[ /* the application's own */ ];
//! static SET: IconSet = IconSet::new(&[rugpui::ICONS, rugpui_shell::WINDOW_CONTROL_ICONS, OWN]);
//! ```
//!
//! Without that chaining gpui's default source answers every path with `None`,
//! which it draws as nothing at all — a column of blank arrow boxes down a tree
//! and a dropdown with no chevron. [`init`](crate::init) says so once, in the
//! log, rather than leaving it to be discovered by squinting at a screenshot.
//!
//! The paths carry a `rugpui/` segment so that a host with a `caret-down.svg`
//! of its own keeps it: the two namespaces cannot collide.
//!
//! What gets drawn is a *monochrome* sprite — resvg rasterises the file, only
//! the alpha channel survives, and the element's `text_color` supplies the
//! colour — so the black stroke written in these two files never reaches the
//! screen, and every widget tints them from the palette instead.
//!
//! The bytes come from [`include_bytes!`], not from files read at run time: a
//! release then carries them wherever it is unpacked, and packaging has nothing
//! extra to ship.

/// The disclosure of something closed: a chevron pointing right.
///
/// Drawn as a stroked chevron in a 24×24 box rather than as a filled triangle,
/// which is what the text glyph `U+25B8` used to supply here. A glyph fills
/// only a fraction of its em square and so was always smaller than the size
/// asked for; an icon is the size it is given.
pub const CARET_RIGHT: &str = "icons/rugpui/caret-right.svg";

/// The disclosure of something open: the same chevron, pointing down.
///
/// Also the mark a dropdown's trigger wears, open or closed — a select's
/// chevron always points down.
pub const CARET_DOWN: &str = "icons/rugpui/caret-down.svg";

/// The two disclosure marks, paired with the bytes an asset source hands back
/// for them.
///
/// A host concatenates this into its own table rather than copying the two
/// files: see the module docs.
pub const ICONS: &[(&str, &[u8])] = &[
    (
        CARET_RIGHT,
        include_bytes!("../assets/icons/caret-right.svg"),
    ),
    (CARET_DOWN, include_bytes!("../assets/icons/caret-down.svg")),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg_in_the_shared_box() {
        for (name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes).expect("an icon must be UTF-8");
            assert!(text.contains("<svg"), "{name} is not an SVG");
            assert!(
                text.contains("viewBox=\"0 0 24 24\""),
                "{name} has the wrong viewBox"
            );
        }
    }

    #[test]
    fn the_paths_are_namespaced_so_a_hosts_own_icons_cannot_collide() {
        for (name, _) in ICONS {
            assert!(
                name.starts_with("icons/rugpui/"),
                "{name} is outside this crate's namespace"
            );
        }
    }
}
