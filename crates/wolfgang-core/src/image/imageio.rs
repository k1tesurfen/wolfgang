//! Decoding through Apple ImageIO: every format macOS can open (HEIC, AVIF,
//! WebP, TIFF, Camera RAW, …), with EXIF orientation applied and RAW developed.

use super::{resize_premultiplied, unpremultiply, ColorTarget, Pixels};
use crate::error::{Error, Result};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURLPathStyle, CFURL, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGColorSpaceModel, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality,
};
use objc2_image_io::{
    kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth, kCGImageSourceCreateThumbnailFromImageAlways,
    kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize, CGImageSource,
};
use std::path::Path;

pub fn decode(path: &Path, max_side: Option<u32>, color: ColorTarget) -> Result<Pixels> {
    let fail = |reason: &str| Error::Decode { path: path.to_path_buf(), reason: reason.into() };

    let path_str = path.to_string_lossy();
    let url = CFURL::with_file_system_path(None, Some(&CFString::from_str(&path_str)), CFURLPathStyle::CFURLPOSIXPathStyle, false)
        .ok_or_else(|| fail("ungültiger Pfad"))?;
    let src = unsafe { CGImageSource::with_url(&url, None) }.ok_or_else(|| fail("Datei lässt sich nicht öffnen"))?;
    let props = unsafe { src.properties_at_index(0, None) }.ok_or_else(|| fail("kein Bild gefunden"))?;
    let props: CFRetained<CFDictionary<CFString, CFType>> = unsafe { CFRetained::cast_unchecked(props) };
    let dim = |key: &CFString| props.get(key).and_then(|v| v.downcast_ref::<CFNumber>().and_then(|n| n.as_i64())).unwrap_or(0);
    let longest = unsafe { dim(kCGImagePropertyPixelWidth).max(dim(kCGImagePropertyPixelHeight)) };
    if longest <= 0 {
        return Err(fail("Bildgröße unbekannt"));
    }

    // The thumbnail API decodes, applies orientation and (for RAW) develops the
    // full image in one go. We always ask for full size and downscale ourselves.
    let yes: &CFType = CFBoolean::new(true).as_ref();
    let size = CFNumber::new_i64(longest);
    let opts = unsafe {
        CFDictionary::<CFString, CFType>::from_slices(
            &[kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize],
            &[yes, yes, size.as_ref()],
        )
    };
    let cg = unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }.ok_or_else(|| fail("Dekodieren fehlgeschlagen"))?;

    let (space, icc) = target_space(&cg, color).ok_or_else(|| fail("Farbraum nicht verfügbar"))?;
    let (width, height, rgba) = render(&cg, &space).ok_or_else(|| fail("Zeichnen fehlgeschlagen"))?;

    let (width, height, mut rgba) = match max_side {
        Some(m) if m < width.max(height) => resize_premultiplied(width, height, rgba, m)?,
        _ => (width, height, rgba),
    };
    unpremultiply(&mut rgba);
    Ok(Pixels { width, height, rgba, icc })
}

/// Picks the colour space to render into, plus the profile to embed (`None` = sRGB).
fn target_space(cg: &CGImage, color: ColorTarget) -> Option<(CFRetained<CGColorSpace>, Option<Vec<u8>>)> {
    let srgb = || CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).map(|s| (s, None));
    if color == ColorTarget::Srgb {
        return srgb();
    }
    let Some(space) = CGImage::color_space(Some(cg)) else { return srgb() };
    if CGColorSpace::model(Some(&space)) != CGColorSpaceModel::RGB {
        return srgb(); // CMYK, grayscale, …: sRGB is the sensible web/master target
    }
    let is_srgb = CGColorSpace::name(Some(&space)).is_some_and(|n| &*n == unsafe { kCGColorSpaceSRGB });
    match CGColorSpace::icc_data(Some(&space)) {
        Some(icc) if !is_srgb => Some((space, Some(icc.to_vec()))),
        _ => srgb(),
    }
}

/// Draws into an 8-bit premultiplied RGBA bitmap; ColorSync converts colours.
fn render(cg: &CGImage, space: &CGColorSpace) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (CGImage::width(Some(cg)), CGImage::height(Some(cg)));
    let mut px = vec![0u8; w.checked_mul(h)?.checked_mul(4)?];
    let ctx = unsafe { CGBitmapContextCreate(px.as_mut_ptr().cast(), w, h, 8, w * 4, Some(space), CGImageAlphaInfo::PremultipliedLast.0) }?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(cg));
    drop(ctx);
    Some((w as u32, h as u32, px))
}
