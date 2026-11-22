//! Images drawn at the size they are shown, rather than shrunk by the GPU.
//!
//! gpui's renderers sample an image with a linear filter and no mipmaps, so
//! an image drawn at a quarter of its size reads one texel in four along each
//! axis: its edges come out stepped, and a thin stroke breaks up. A 64 px
//! product logo shown at 14 px is exactly that case.
//!
//! [`scaled_image`] resamples the image once, on the CPU, to the device
//! pixels it will cover — a Lanczos filter, which averages every source pixel
//! under each one it writes, run over premultiplied alpha so that the
//! transparent pixels around a mark do not darken its edge — and gives gpui
//! that bitmap to draw one to one. Each size is kept, by the asset and the
//! pixels it covers, so a window moved to a display of another scale makes one
//! more and keeps the first.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, Global, ImageSource, Pixels, RenderImage, SharedString, Window};
use image::imageops::{self, FilterType};
use image::{Frame, Rgba32FImage, RgbaImage};

/// Every image [`scaled_image`] has made, by asset path and edge length in
/// device pixels; `None` for an asset that could not be loaded or decoded,
/// so that it is not tried again on every frame.
#[derive(Default)]
struct Scaled(HashMap<(SharedString, u32), Option<Arc<RenderImage>>>);

impl Global for Scaled {}

/// The image at the asset `path`, fitted into a square of `size` on
/// `window` and resampled to the device pixels that square covers, for
/// [`img`](gpui::img) to draw one to one.
///
/// The image keeps its aspect ratio: its longer side is `size`. An asset the
/// application's [`AssetSource`](gpui::AssetSource) does not have, or that
/// does not decode, is handed to gpui as the path, to load and draw as
/// `img(path)` would.
pub fn scaled_image(
    path: &SharedString,
    size: Pixels,
    window: &Window,
    cx: &mut App,
) -> ImageSource {
    let edge = (f32::from(size) * window.scale_factor()).round().max(1.) as u32;
    let key = (path.clone(), edge);
    let made = match cx.default_global::<Scaled>().0.get(&key) {
        Some(made) => made.clone(),
        None => {
            let made = cx
                .asset_source()
                .load(path)
                .ok()
                .flatten()
                .and_then(|bytes| resample(&bytes, edge));
            cx.default_global::<Scaled>().0.insert(key, made.clone());
            made
        }
    };
    match made {
        Some(image) => ImageSource::Render(image),
        None => path.clone().into(),
    }
}

/// `bytes` decoded, fitted into `edge` × `edge` and resampled to it, as the
/// BGRA frame gpui draws.
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
}
