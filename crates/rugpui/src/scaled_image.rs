//! Images drawn at the size they are shown, rather than shrunk by the GPU.
//!
//! gpui's renderers sample an image with a linear filter and no mipmaps, so
//! an image drawn at a quarter of its size reads one texel in four along each
//! axis: its edges come out stepped, and a thin stroke breaks up. A 64 px
//! product logo shown at 14 px is exactly that case.
//!
//! [`scaled_image`] makes the image once, on the CPU, at the device pixels it
//! will cover, and gives gpui that bitmap to draw one to one. A raster image is
//! resampled with a Lanczos filter, which averages every source pixel under
//! each one it writes, over premultiplied alpha so that the transparent pixels
//! around a mark do not darken its edge; an SVG is rasterized at that size.
//! Each size is kept, by the image and the pixels it covers, so a window moved
//! to a display of another scale makes one more and keeps the first.
//!
//! An image is an asset of the application's [`AssetSource`](gpui::AssetSource)
//! or a file on disk ([`ImagePath`]). A file is read again once it changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use gpui::{
    App, DevicePixels, Global, ImageSource, Pixels, RenderImage, SharedString, SvgSize, Window,
    size,
};
use image::imageops::{self, FilterType};
use image::{Frame, Rgba32FImage, RgbaImage};

/// Where an image is: an asset of the application, or a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImagePath {
    /// A path of the application's [`AssetSource`](gpui::AssetSource).
    Asset(SharedString),
    /// A file on disk.
    File(Arc<Path>),
}

impl From<SharedString> for ImagePath {
    fn from(path: SharedString) -> Self {
        Self::Asset(path)
    }
}

impl From<&'static str> for ImagePath {
    fn from(path: &'static str) -> Self {
        Self::Asset(path.into())
    }
}

impl From<String> for ImagePath {
    fn from(path: String) -> Self {
        Self::Asset(path.into())
    }
}

impl From<PathBuf> for ImagePath {
    fn from(path: PathBuf) -> Self {
        Self::File(path.into())
    }
}

impl From<&Path> for ImagePath {
    fn from(path: &Path) -> Self {
        Self::File(path.into())
    }
}

/// An image's version: a file's modification time, so that an edited file is
/// read again; nothing for an asset, which cannot change.
type Version = Option<SystemTime>;

/// What [`scaled_image`] and [`image_loads`] have made, by the image, its
/// version and the edge in device pixels; `None` for an image that could not
/// be read or decoded, so that it is not tried again on every frame.
#[derive(Default)]
struct Scaled {
    made: HashMap<(ImagePath, Version, u32), Option<Arc<RenderImage>>>,
    loads: HashMap<(ImagePath, Version), bool>,
}

impl Global for Scaled {}

/// The image at `path`, fitted into a square of `size` on `window` and made
/// at the device pixels that square covers, for [`img`](gpui::img) to draw
/// one to one; `None` for one that cannot be read or decoded.
///
/// The image keeps its aspect ratio: its longer side is `size`. A raster
/// image is anything gpui decodes — PNG, JPEG, WebP, GIF's first frame, BMP,
/// ICO — and an SVG is told apart by its text.
pub fn scaled_image(
    path: &ImagePath,
    size: Pixels,
    window: &Window,
    cx: &mut App,
) -> Option<ImageSource> {
    let edge = (f32::from(size) * window.scale_factor()).round().max(1.) as u32;
    let key = (path.clone(), version(path), edge);
    if let Some(made) = cx.default_global::<Scaled>().made.get(&key) {
        return made.clone().map(ImageSource::Render);
    }
    let made = bytes(path, cx).and_then(|bytes| make(&bytes, edge, cx));
    cx.default_global::<Scaled>().made.insert(key, made.clone());
    made.map(ImageSource::Render)
}

/// Whether the image at `path` can be read and decoded: what a caller with a
/// fallback asks before it settles on one.
pub fn image_loads(path: &ImagePath, cx: &mut App) -> bool {
    let key = (path.clone(), version(path));
    if let Some(loads) = cx.default_global::<Scaled>().loads.get(&key) {
        return *loads;
    }
    let loads = bytes(path, cx).is_some_and(|bytes| {
        if is_svg(&bytes) {
            cx.svg_renderer().parse_svg(&bytes).is_ok()
        } else {
            image::load_from_memory(&bytes).is_ok()
        }
    });
    cx.default_global::<Scaled>().loads.insert(key, loads);
    loads
}

fn version(path: &ImagePath) -> Version {
    match path {
        ImagePath::Asset(_) => None,
        ImagePath::File(file) => std::fs::metadata(file)
            .and_then(|meta| meta.modified())
            .ok(),
    }
}

fn bytes(path: &ImagePath, cx: &App) -> Option<Vec<u8>> {
    match path {
        ImagePath::Asset(asset) => cx
            .asset_source()
            .load(asset)
            .ok()
            .flatten()
            .map(|bytes| bytes.into_owned()),
        ImagePath::File(file) => std::fs::read(file).ok(),
    }
}

/// Whether `bytes` are SVG text rather than a raster image.
fn is_svg(bytes: &[u8]) -> bool {
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = text.iter().position(|byte| !byte.is_ascii_whitespace());
    start.is_some_and(|at| text[at] == b'<')
}

/// `bytes` fitted into `edge` × `edge`, as the BGRA frame gpui draws.
fn make(bytes: &[u8], edge: u32, cx: &App) -> Option<Arc<RenderImage>> {
    if is_svg(bytes) {
        rasterize(bytes, edge, cx)
    } else {
        resample(bytes, edge)
    }
}

/// An SVG rasterized to fit `edge` × `edge`: once at its own size, for its
/// aspect ratio, then at the width that fits.
fn rasterize(bytes: &[u8], edge: u32, cx: &App) -> Option<Arc<RenderImage>> {
    let renderer = cx.svg_renderer();
    let svg = renderer.parse_svg(bytes).ok()?;
    let natural = renderer.render_parsed(&svg, 1.0).ok()?.size(0);
    let (width, height) = (
        natural.width.0.max(1) as f32,
        natural.height.0.max(1) as f32,
    );
    let fit = if width >= height {
        edge
    } else {
        ((edge as f32 * width / height).round() as u32).max(1)
    };
    let fit = DevicePixels(fit as i32);
    renderer
        .render_parsed(&svg, SvgSize::Size(size(fit, fit)))
        .ok()
}

/// A raster image decoded, fitted into `edge` × `edge` and resampled to it.
fn resample(bytes: &[u8], edge: u32) -> Option<Arc<RenderImage>> {
    let mut pixels = image::load_from_memory(bytes).ok()?.into_rgba32f();
    let (width, height) = pixels.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let scale = edge as f32 / width.max(height) as f32;
    let target = |side: u32| ((side as f32 * scale).round() as u32).max(1);
    let (to_width, to_height) = (target(width), target(height));
    if (to_width, to_height) != (width, height) {
        premultiply(&mut pixels, true);
        pixels = imageops::resize(&pixels, to_width, to_height, FilterType::Lanczos3);
        premultiply(&mut pixels, false);
    }
    let mut data = RgbaImage::new(to_width, to_height);
    for (out, pixel) in data.pixels_mut().zip(pixels.pixels()) {
        let [r, g, b, a] = pixel
            .0
            .map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8);
        // gpui keeps an image as BGRA with straight alpha.
        out.0 = [b, g, r, a];
    }
    Some(Arc::new(RenderImage::new([Frame::new(data)])))
}

/// Multiplies each pixel's color by its alpha, or with `forward` false
/// divides it back out.
fn premultiply(pixels: &mut Rgba32FImage, forward: bool) {
    for pixel in pixels.pixels_mut() {
        let alpha = pixel.0[3].clamp(0., 1.);
        for channel in &mut pixel.0[..3] {
            *channel = if forward {
                *channel * alpha
            } else if alpha > 0. {
                *channel / alpha
            } else {
                0.
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of `size` × `size` whose left half is opaque red and right half
    /// transparent black.
    fn half_red(size: u32) -> Vec<u8> {
        let image = RgbaImage::from_fn(size, size, |x, _| {
            if x < size / 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encodes");
        bytes
    }

    #[test]
    fn an_image_is_resampled_to_the_edge_it_covers_as_bgra() {
        let image = resample(&half_red(64), 16).expect("decodes");
        let frame = image.as_bytes(0).expect("one frame");
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (16, 16));
        // BGRA: red is the third byte.
        assert_eq!(&frame[..4], &[0, 0, 255, 255], "opaque red, swapped");
    }

    /// The pixels beside the transparent half stay red as they fade, rather
    /// than going dark: the alpha is premultiplied while they are averaged.
    #[test]
    fn a_transparent_edge_does_not_darken_the_mark() {
        let image = resample(&half_red(64), 15).expect("decodes");
        let frame = image.as_bytes(0).expect("one frame");
        let row = &frame[..15 * 4];
        for pixel in row.as_chunks::<4>().0 {
            let [b, g, r, a] = *pixel;
            if a > 16 {
                assert!(r > 200 && g < 40 && b < 40, "a darkened edge: {pixel:?}");
            }
        }
    }

    #[test]
    fn a_wide_image_keeps_its_aspect_ratio() {
        let wide = RgbaImage::from_pixel(64, 32, image::Rgba([0, 0, 255, 255]));
        let mut bytes = Vec::new();
        wide.write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encodes");
        let image = resample(&bytes, 20).expect("decodes");
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (20, 10));
    }

    #[test]
    fn bytes_that_are_no_image_give_none() {
        assert!(resample(b"not an image", 16).is_none());
    }

    #[test]
    fn svg_text_is_told_apart_from_a_raster_image() {
        assert!(is_svg(b"  <svg xmlns='http://www.w3.org/2000/svg'/>"));
        assert!(is_svg(b"\xEF\xBB\xBF<?xml version='1.0'?><svg/>"));
        assert!(!is_svg(&half_red(4)));
    }

    const SQUARE_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='20' \
                              viewBox='0 0 10 20'><rect width='10' height='20' fill='#00f'/></svg>";

    /// A file is read, rasterized or resampled to the pixels it covers, read
    /// again once it changes, and answers `None` once it is gone.
    #[gpui::test]
    fn a_file_is_drawn_at_its_size_and_followed_as_it_changes(cx: &mut gpui::TestAppContext) {
        use gpui::{VisualTestContext, div, prelude::*, px};

        struct Empty;
        impl Render for Empty {
            fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
                div()
            }
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let png = dir.path().join("logo.png");
        std::fs::write(&png, half_red(64)).expect("write");
        let svg = dir.path().join("logo.svg");
        std::fs::write(&svg, SQUARE_SVG).expect("write");
        let window = cx.open_window(gpui::size(px(100.), px(100.)), |_, _| Empty);
        let mut cx = VisualTestContext::from_window(*std::ops::Deref::deref(&window), cx);

        cx.update(|window, cx| {
            let scale = window.scale_factor();
            let edge = (16. * scale).round() as i32;
            let drawn = |path: &ImagePath, window: &Window, cx: &mut App| match scaled_image(
                path,
                px(16.),
                window,
                cx,
            ) {
                Some(ImageSource::Render(image)) => image.size(0),
                other => panic!("{path:?}: not a rendered image ({})", other.is_some()),
            };

            let png = ImagePath::from(png.clone());
            assert!(image_loads(&png, cx));
            let size = drawn(&png, window, cx);
            assert_eq!((size.width.0, size.height.0), (edge, edge));

            let svg_path = ImagePath::from(svg.clone());
            assert!(image_loads(&svg_path, cx));
            let size = drawn(&svg_path, window, cx);
            assert_eq!(size.height.0, edge, "the taller side fits");
            assert_eq!(size.width.0, (edge as f32 / 2.).round() as i32);

            let text = ImagePath::from(dir.path().join("notes.txt"));
            std::fs::write(dir.path().join("notes.txt"), "no image").expect("write");
            assert!(!image_loads(&text, cx));
            assert!(scaled_image(&text, px(16.), window, cx).is_none());

            std::fs::remove_file(svg.as_path()).expect("remove");
            assert!(!image_loads(&svg_path, cx), "a file that is gone");
            assert!(
                !image_loads(&ImagePath::from("icons/none.png"), cx),
                "an asset nobody has"
            );
        });
    }
}
