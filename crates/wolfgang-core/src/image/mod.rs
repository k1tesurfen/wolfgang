//! Image conversion: decode (ImageIO on macOS) → resize → encode.

mod encode;
#[cfg(target_os = "macos")]
mod imageio;

use crate::error::{Error, Result};
use crate::settings::ImageFormat;
use std::path::Path;

pub use encode::encode;

/// Straight (non-premultiplied) 8-bit RGBA pixels.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// ICC profile of the pixel data; `None` means sRGB.
    pub icc: Option<Vec<u8>>,
}

impl Pixels {
    pub fn has_alpha(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] != 255)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTarget {
    /// Convert to sRGB (web mode, and formats that can't carry a profile).
    Srgb,
    /// Keep the source's RGB colour space (e.g. Display P3) and embed its profile.
    KeepSource,
}

/// What to do with one image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageTask {
    pub format: ImageFormat,
    /// Longest side; `None` keeps the original size. Never upscales.
    pub max_side: Option<u32>,
    /// Encoder-native quality (ignored for PNG).
    pub quality: f32,
    pub color: ColorTarget,
}

/// Decodes with orientation applied, scaled to `max_side` if smaller than the original.
pub fn decode(path: &Path, max_side: Option<u32>, color: ColorTarget) -> Result<Pixels> {
    #[cfg(target_os = "macos")]
    {
        imageio::decode(path, max_side, color)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (max_side, color);
        Err(Error::Decode { path: path.to_path_buf(), reason: "Bilder lesen geht bisher nur auf macOS".into() })
    }
}

pub fn convert(path: &Path, task: &ImageTask) -> Result<Vec<u8>> {
    // ravif always writes sRGB primaries, so AVIF can't carry another profile.
    let color = if task.format == ImageFormat::Avif { ColorTarget::Srgb } else { task.color };
    let pixels = decode(path, task.max_side, color)?;
    encode(&pixels, task.format, task.quality)
}

/// Lanczos3 downscale of *premultiplied* RGBA (correct for transparent edges).
pub(crate) fn resize_premultiplied(width: u32, height: u32, rgba: Vec<u8>, longest: u32) -> Result<(u32, u32, Vec<u8>)> {
    use fast_image_resize::{images::Image, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
    let scale = longest as f64 / width.max(height) as f64;
    let nw = ((width as f64 * scale).round() as u32).max(1);
    let nh = ((height as f64 * scale).round() as u32).max(1);
    let resize_err = |e: &dyn std::fmt::Display| Error::Encode { format: "resize", reason: e.to_string() };
    let src = Image::from_vec_u8(width, height, rgba, PixelType::U8x4).map_err(|e| resize_err(&e))?;
    let mut dst = Image::new(nw, nh, PixelType::U8x4);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)).use_alpha(false);
    Resizer::new().resize(&src, &mut dst, &opts).map_err(|e| resize_err(&e))?;
    Ok((nw, nh, dst.into_vec()))
}

pub(crate) fn unpremultiply(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}
