//! Encoders, all statically linked: mozjpeg, libwebp, rav1e (via ravif), png.
//! Nothing but pixels and (optionally) an ICC profile is written: no EXIF, no GPS.

use super::Pixels;
use crate::error::{Error, Result};
use crate::settings::ImageFormat;

pub fn encode(p: &Pixels, format: ImageFormat, quality: f32) -> Result<Vec<u8>> {
    let err = |reason: String| Error::Encode { format: format.label(), reason };
    match format {
        ImageFormat::Jpg => jpeg(p, quality).map_err(|e| err(e.to_string())),
        ImageFormat::Webp => webp(p, quality).map_err(err),
        ImageFormat::Avif => avif(p, quality).map_err(|e| err(e.to_string())),
        ImageFormat::Png => png(p).map_err(|e| err(e.to_string())),
    }
}

fn jpeg(p: &Pixels, quality: f32) -> std::io::Result<Vec<u8>> {
    // No alpha in JPEG: composite over white.
    let rgb: Vec<u8> = p
        .rgba
        .chunks_exact(4)
        .flat_map(|px| {
            let a = px[3] as u32;
            [0, 1, 2].map(|i| ((px[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8)
        })
        .collect();
    let mut comp = mozjpeg::Compress::new(mozjpeg::ColorSpace::JCS_RGB);
    comp.set_size(p.width as usize, p.height as usize);
    comp.set_quality(quality);
    comp.set_progressive_mode();
    comp.set_optimize_coding(true);
    let mut c = comp.start_compress(Vec::new())?;
    if let Some(icc) = &p.icc {
        c.write_icc_profile(icc);
    }
    c.write_scanlines(&rgb)?;
    c.finish()
}

fn webp(p: &Pixels, quality: f32) -> std::result::Result<Vec<u8>, String> {
    let enc = webp::Encoder::from_rgba(&p.rgba, p.width, p.height);
    let mut cfg = webp::WebPConfig::new().map_err(|_| "WebP-Konfiguration".to_string())?;
    cfg.quality = quality;
    cfg.method = 5;
    let data = enc.encode_advanced(&cfg).map_err(|e| format!("{e:?}"))?.to_vec();
    Ok(match &p.icc {
        Some(icc) => webp_with_icc(&data, icc, p.width, p.height, p.has_alpha()),
        None => data,
    })
}

/// Wraps a simple-format WebP (`VP8 `/`VP8L` or `VP8X`) into the extended
/// format with an `ICCP` chunk, as the WebP container spec requires.
fn webp_with_icc(data: &[u8], icc: &[u8], width: u32, height: u32, alpha: bool) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(tag);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
    }
    // Collect existing chunks after "RIFF....WEBP".
    let mut chunks: Vec<([u8; 4], &[u8])> = Vec::new();
    let mut i = 12;
    while i + 8 <= data.len() {
        let tag: [u8; 4] = data[i..i + 4].try_into().unwrap();
        let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap()) as usize;
        let end = (i + 8 + len).min(data.len());
        chunks.push((tag, &data[i + 8..end]));
        i = end + (len % 2);
    }
    let mut flags = 0x20u8; // ICC
    if alpha || chunks.iter().any(|(t, _)| t == b"ALPH") {
        flags |= 0x10;
    }
    let mut vp8x = vec![flags, 0, 0, 0];
    vp8x.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x.extend_from_slice(&(height - 1).to_le_bytes()[..3]);

    let mut body = b"WEBP".to_vec();
    chunk(&mut body, b"VP8X", &vp8x);
    chunk(&mut body, b"ICCP", icc);
    for (tag, payload) in chunks.iter().filter(|(t, _)| t != b"VP8X" && t != b"ICCP") {
        chunk(&mut body, tag, payload);
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn avif(p: &Pixels, quality: f32) -> std::result::Result<Vec<u8>, ravif::Error> {
    use rgb::FromSlice;
    let enc = ravif::Encoder::new().with_quality(quality).with_alpha_quality(quality).with_speed(6);
    let img = ravif::Img::new(p.rgba.as_rgba(), p.width as usize, p.height as usize);
    Ok(enc.encode_rgba(img)?.avif_file)
}

fn png(p: &Pixels) -> std::result::Result<Vec<u8>, png::EncodingError> {
    let alpha = p.has_alpha();
    let mut info = png::Info::with_size(p.width, p.height);
    info.color_type = if alpha { png::ColorType::Rgba } else { png::ColorType::Rgb };
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = p.icc.as_deref().map(Into::into);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::with_info(&mut out, info)?;
        enc.set_compression(png::Compression::High);
        let mut w = enc.write_header()?;
        if alpha {
            w.write_image_data(&p.rgba)?;
        } else {
            let rgb: Vec<u8> = p.rgba.chunks_exact(4).flat_map(|px| [px[0], px[1], px[2]]).collect();
            w.write_image_data(&rgb)?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, alpha: bool) -> Pixels {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 128, if alpha { (x * 255 / w) as u8 } else { 255 }]);
            }
        }
        Pixels { width: w, height: h, rgba, icc: None }
    }

    #[test]
    fn every_format_produces_its_signature() {
        let p = gradient(64, 48, true);
        assert_eq!(&encode(&p, ImageFormat::Jpg, 80.0).unwrap()[..3], &[0xFF, 0xD8, 0xFF]);
        let w = encode(&p, ImageFormat::Webp, 80.0).unwrap();
        assert_eq!((&w[..4], &w[8..12]), (&b"RIFF"[..], &b"WEBP"[..]));
        assert_eq!(&encode(&p, ImageFormat::Avif, 70.0).unwrap()[4..12], b"ftypavif");
        assert_eq!(&encode(&p, ImageFormat::Png, 0.0).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn webp_icc_container_is_well_formed() {
        let mut p = gradient(33, 17, false);
        p.icc = Some(vec![7; 101]); // odd length exercises padding
        let w = encode(&p, ImageFormat::Webp, 80.0).unwrap();
        let riff_len = u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize;
        assert_eq!(riff_len + 8, w.len());
        assert_eq!(&w[12..16], b"VP8X");
        assert_eq!(w[20] & 0x20, 0x20, "ICC flag");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], 0]) + 1, 33);
        assert_eq!(u32::from_le_bytes([w[27], w[28], w[29], 0]) + 1, 17);
        assert_eq!(&w[30..34], b"ICCP");
        assert_eq!(u32::from_le_bytes(w[34..38].try_into().unwrap()), 101);
    }
}
