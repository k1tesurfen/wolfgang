//! Bundling spike: prove that one self-contained arm64 binary can
//! decode anything ImageIO reads (HEIC, AVIF, RAW, ...), resize, convert to sRGB,
//! encode JPG/WebP/AVIF/PNG, and subset + compress fonts to WOFF2.
//!
//! Usage:
//!   wolfgang-spike img <in> <out.{jpg,webp,avif,png}> [max_side|0] [quality]
//!   wolfgang-spike font <in.{ttf,otf}> <out.woff2> [de|ext|all]

use anyhow::{anyhow, bail, Context, Result};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURLPathStyle, CFURL};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGInterpolationQuality, kCGColorSpaceSRGB};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_image_io::{
    kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth, kCGImageSourceCreateThumbnailFromImageAlways,
    kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize, CGImageSource,
};
use std::path::Path;
use std::time::Instant;

/// Straight (non-premultiplied) sRGB RGBA8 pixels.
struct Rgba {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("img") => {
            let max_side: u32 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(0);
            let quality: f32 = args.get(5).map(|s| s.parse()).transpose()?.unwrap_or(80.0);
            img(Path::new(&args[2]), Path::new(&args[3]), max_side, quality)
        }
        Some("font") => font(Path::new(&args[2]), Path::new(&args[3]), args.get(4).map(String::as_str).unwrap_or("de")),
        _ => bail!("usage: img <in> <out> [max_side] [quality] | font <in> <out> [de|ext|all]"),
    }
}

fn img(input: &Path, output: &Path, max_side: u32, quality: f32) -> Result<()> {
    let t = Instant::now();
    let rgba = decode(input, max_side)?;
    let t_dec = t.elapsed();
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let bytes = match ext.as_str() {
        "jpg" | "jpeg" => encode_jpeg(&rgba, quality)?,
        "webp" => encode_webp(&rgba, quality)?,
        "avif" => encode_avif(&rgba, quality)?,
        "png" => encode_png(&rgba)?,
        _ => bail!("unknown output format {ext}"),
    };
    std::fs::write(output, &bytes)?;
    println!(
        "{} → {}  {}x{}  {} KB → {} KB  (decode {:?}, total {:?})",
        input.display(),
        output.display(),
        rgba.w,
        rgba.h,
        std::fs::metadata(input)?.len() / 1024,
        bytes.len() / 1024,
        t_dec,
        t.elapsed()
    );
    Ok(())
}

/// Decode via ImageIO: applies EXIF orientation, develops RAW, scales so the
/// longest side is at most `max_side` (0 = original), and renders into sRGB.
fn decode(path: &Path, max_side: u32) -> Result<Rgba> {
    let path_str = path.canonicalize()?.to_string_lossy().into_owned();
    let url = CFURL::with_file_system_path(None, Some(&CFString::from_str(&path_str)), CFURLPathStyle::CFURLPOSIXPathStyle, false)
        .ok_or_else(|| anyhow!("bad path"))?;
    let src = unsafe { CGImageSource::with_url(&url, None) }.ok_or_else(|| anyhow!("ImageIO cannot open {path_str}"))?;

    let props = unsafe { src.properties_at_index(0, None) }.ok_or_else(|| anyhow!("no image in {path_str}"))?;
    let props: CFRetained<CFDictionary<CFString, CFType>> = unsafe { CFRetained::cast_unchecked(props) };
    let dim = |k: &CFString| props.get(k).and_then(|v| v.downcast_ref::<CFNumber>().and_then(|n| n.as_i64())).unwrap_or(0) as u32;
    let (ow, oh) = unsafe { (dim(kCGImagePropertyPixelWidth), dim(kCGImagePropertyPixelHeight)) };
    let longest = ow.max(oh);
    let target = if max_side == 0 || max_side > longest { longest } else { max_side };

    // Thumbnail API = decode + orientation + (for RAW) full develop in one step.
    // We always decode at full size and do the downscale ourselves with Lanczos.
    let yes: &CFType = CFBoolean::new(true).as_ref();
    let size = CFNumber::new_i64(longest as i64);
    let opts = unsafe {
        CFDictionary::<CFString, CFType>::from_slices(
            &[kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize],
            &[yes, yes, size.as_ref()],
        )
    };
    let cg = unsafe { src.thumbnail_at_index(0, Some(opts.as_opaque())) }.ok_or_else(|| anyhow!("decode failed for {path_str}"))?;
    let full = render_srgb(&cg)?;

    Ok(if target < longest { resize(full, target)? } else { unpremultiply(full) })
}

/// Draw the CGImage into an 8-bit premultiplied sRGB bitmap (ColorSync does the profile conversion).
fn render_srgb(cg: &CGImage) -> Result<Rgba> {
    let (w, h) = (CGImage::width(Some(cg)), CGImage::height(Some(cg)));
    let mut px = vec![0u8; w * h * 4];
    let cs = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or_else(|| anyhow!("no sRGB"))?;
    let ctx = unsafe { CGBitmapContextCreate(px.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&cs), CGImageAlphaInfo::PremultipliedLast.0) }
        .ok_or_else(|| anyhow!("bitmap context failed"))?;
    CGContext::set_interpolation_quality(Some(&ctx), CGInterpolationQuality::High);
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)), Some(cg));
    drop(ctx);
    Ok(Rgba { w: w as u32, h: h as u32, px })
}

/// Lanczos3 downscale on premultiplied data (correct for alpha), then unpremultiply.
fn resize(src: Rgba, longest: u32) -> Result<Rgba> {
    use fast_image_resize::{images::Image, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
    let scale = longest as f64 / src.w.max(src.h) as f64;
    let (nw, nh) = (((src.w as f64 * scale).round() as u32).max(1), ((src.h as f64 * scale).round() as u32).max(1));
    let s = Image::from_vec_u8(src.w, src.h, src.px, PixelType::U8x4)?;
    let mut d = Image::new(nw, nh, PixelType::U8x4);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)).use_alpha(false);
    Resizer::new().resize(&s, &mut d, &opts)?;
    Ok(unpremultiply(Rgba { w: nw, h: nh, px: d.into_vec() }))
}

fn unpremultiply(mut img: Rgba) -> Rgba {
    for p in img.px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    img
}

fn has_alpha(img: &Rgba) -> bool {
    img.px.chunks_exact(4).any(|p| p[3] != 255)
}

fn encode_jpeg(img: &Rgba, q: f32) -> Result<Vec<u8>> {
    // JPEG has no alpha: composite over white.
    let rgb: Vec<u8> = img
        .px
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            [0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8)
        })
        .collect();
    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    comp.set_size(img.w as usize, img.h as usize);
    comp.set_quality(q);
    comp.set_progressive_mode();
    let mut c = comp.start_compress(Vec::new())?;
    c.write_scanlines(&rgb)?;
    Ok(c.finish()?)
}

fn encode_webp(img: &Rgba, q: f32) -> Result<Vec<u8>> {
    let enc = webp::Encoder::from_rgba(&img.px, img.w, img.h);
    Ok(enc.encode(q).to_vec())
}

fn encode_avif(img: &Rgba, q: f32) -> Result<Vec<u8>> {
    use rgb::FromSlice;
    let enc = ravif::Encoder::new().with_quality(q).with_alpha_quality(q).with_speed(6);
    let res = enc.encode_rgba(ravif::Img::new(img.px.as_rgba(), img.w as usize, img.h as usize))?;
    Ok(res.avif_file)
}

fn encode_png(img: &Rgba) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.w, img.h);
    enc.set_compression(png::Compression::High);
    if has_alpha(img) {
        enc.set_color(png::ColorType::Rgba);
        enc.write_header()?.write_image_data(&img.px)?;
    } else {
        enc.set_color(png::ColorType::Rgb);
        let rgb: Vec<u8> = img.px.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        enc.write_header()?.write_image_data(&rgb)?;
    }
    Ok(out)
}

fn font(input: &Path, output: &Path, subset: &str) -> Result<()> {
    use hb_subset::{Blob, FontFace, SubsetInput};
    let data = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let sfnt = if subset == "all" {
        data.clone()
    } else {
        let face = FontFace::new(Blob::from_bytes(&data)?)?;
        let mut si = SubsetInput::new()?;
        {
            let mut set = si.unicode_set();
            set.insert_range('\u{0020}'..='\u{007E}'); // Basic Latin
            set.insert_range('\u{00A0}'..='\u{00FF}'); // Latin-1 (äöüß…)
            set.insert_range('\u{2013}'..='\u{2026}'); // dashes, quotes, bullet, ellipsis
            set.insert('\u{20AC}'); // €
            if subset == "ext" {
                set.insert_range('\u{0100}'..='\u{017F}'); // Latin Extended-A
            }
        }
        let out = si.subset_font(&face)?;
        let bytes = out.underlying_blob().to_vec();
        bytes
    };
    let woff2 = ttf2woff2::encode(&sfnt, ttf2woff2::BrotliQuality::default()).map_err(|e| anyhow!("woff2: {e:?}"))?;
    std::fs::write(output, &woff2)?;
    println!("{} → {}  {} KB → {} KB (subset: {subset})", input.display(), output.display(), data.len() / 1024, woff2.len() / 1024);
    Ok(())
}
