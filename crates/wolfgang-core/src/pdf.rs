//! Pulling the pictures out of a PDF, as they are: JPEGs stay JPEGs (copied
//! byte for byte), everything else becomes PNG. Nothing is rendered.

use crate::error::{Error, Result};
use crate::image::{self, ColorTarget, Pixels};
use crate::settings::ImageFormat;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Smaller than this (either side) is a line, a bullet or an icon, not a picture.
const MIN_SIDE: i64 = 48;
/// Decoded pixel data larger than this is refused (broken or hostile files).
const MAX_PIXELS: i64 = 200_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub page: u32,
    /// 1, 2, … within the page.
    pub number: u32,
    pub data: Vec<u8>,
    /// `jpg` or `png`
    pub ext: &'static str,
}

#[derive(Debug, Default)]
pub struct Extraction {
    pub pictures: Vec<Picture>,
    pub pages: u32,
    /// Pictures in formats Wolfgang can't read (fax-style scans, special colour spaces).
    pub unsupported: usize,
}

pub fn extract(path: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<Extraction> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let doc = Document::load(path).map_err(|e| match e {
        lopdf::Error::Decryption(_) | lopdf::Error::InvalidPassword => Error::Pdf(format!("{name} ist mit einem Passwort geschützt")),
        other => Error::Pdf(format!("Kann {name} nicht als PDF lesen ({other})")),
    })?;
    let pages = doc.get_pages();
    let mut out = Extraction { pages: pages.len() as u32, ..Default::default() };
    let mut seen_objects = HashSet::new();
    let mut seen_content = HashSet::new();
    for (i, (&page_no, &page_id)) in pages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut ids = Vec::new();
        if let Ok(page) = doc.get_dictionary(page_id) {
            collect_images(&doc, page_resources(&doc, page), &mut ids, &mut HashSet::new(), 0);
        }
        let mut number = 0;
        for id in ids {
            if !seen_objects.insert(id) {
                continue; // the same logo on every page
            }
            let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else { continue };
            match picture(&doc, stream) {
                Ok(Some((data, ext))) => {
                    if seen_content.insert(blake3::hash(&data)) {
                        number += 1;
                        out.pictures.push(Picture { page: page_no, number, data, ext });
                    }
                }
                Ok(None) => {}
                Err(_) => out.unsupported += 1,
            }
        }
        progress((i + 1) as f64 / pages.len().max(1) as f64);
    }
    Ok(out)
}

/// A page's resources, inherited from parent page-tree nodes if needed.
fn page_resources<'a>(doc: &'a Document, page: &'a Dictionary) -> Option<&'a Dictionary> {
    let mut node = page;
    for _ in 0..32 {
        if let Ok(r) = node.get(b"Resources") {
            return doc.dereference(r).ok()?.1.as_dict().ok();
        }
        node = doc.get_dictionary(node.get(b"Parent").ok()?.as_reference().ok()?).ok()?;
    }
    None
}

/// Image XObjects, also those inside form XObjects (layout programs nest them).
fn collect_images(doc: &Document, resources: Option<&Dictionary>, ids: &mut Vec<ObjectId>, forms: &mut HashSet<ObjectId>, depth: u32) {
    let Some(xobjects) = resources.and_then(|r| r.get(b"XObject").ok()).and_then(|x| doc.dereference(x).ok()).and_then(|(_, x)| x.as_dict().ok()) else {
        return;
    };
    for (_, obj) in xobjects.iter() {
        let Ok(id) = obj.as_reference() else { continue };
        let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) else { continue };
        match stream.dict.get(b"Subtype").and_then(Object::as_name) {
            Ok(b"Image") => ids.push(id),
            Ok(b"Form") if depth < 8 && forms.insert(id) => {
                let inner = stream.dict.get(b"Resources").ok().and_then(|r| doc.dereference(r).ok()).and_then(|(_, r)| r.as_dict().ok());
                collect_images(doc, inner, ids, forms, depth + 1);
            }
            _ => {}
        }
    }
}

fn int(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<i64> {
    doc.dereference(d.get(key).ok()?).ok()?.1.as_i64().ok()
}

fn last_filter(stream: &Stream) -> Option<Vec<u8>> {
    stream.filters().ok()?.last().map(|f| f.to_vec())
}

/// One picture as file data, `None` if it's too small to matter.
fn picture(doc: &Document, stream: &Stream) -> std::result::Result<Option<(Vec<u8>, &'static str)>, String> {
    let d = &stream.dict;
    if d.get(b"ImageMask").and_then(Object::as_bool).unwrap_or(false) {
        return Ok(None); // a stencil (text-like shape), not a picture
    }
    let (w, h) = (int(doc, d, b"Width").ok_or("ohne Breite")?, int(doc, d, b"Height").ok_or("ohne Höhe")?);
    if w < MIN_SIDE || h < MIN_SIDE {
        return Ok(None);
    }
    if w * h > MAX_PIXELS {
        return Err("zu groß".into());
    }
    let space = color_space(doc, d.get(b"ColorSpace").ok())?;
    let alpha = soft_mask(doc, d, w as u32, h as u32);
    let filter = last_filter(stream);
    let single_filter = stream.filters().map(|f| f.len() == 1).unwrap_or(false);
    let mut px = match filter.as_deref() {
        // JPEG: copied as it is, unless it needs fixing (CMYK, transparency).
        Some(b"DCTDecode") if single_filter => {
            if alpha.is_none() && !matches!(space.base(), Space::Cmyk) {
                return Ok(Some((stream.content.clone(), "jpg")));
            }
            decode_with_imageio(&stream.content, "jpg")?
        }
        Some(b"JPXDecode") if single_filter => decode_with_imageio(&stream.content, "jp2")?,
        Some(b"DCTDecode" | b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode") => return Err("Format nicht unterstützt".into()),
        _ => {
            let bpc = int(doc, d, b"BitsPerComponent").unwrap_or(8) as u32;
            let limit = (w * h * 4 * 2 + 4096) as usize;
            let raw = stream.decompressed_content_with_limit(limit).map_err(|e| e.to_string())?;
            let invert = decode_inverted(doc, d);
            Pixels { width: w as u32, height: h as u32, rgba: to_rgba(&raw, w as u32, h as u32, bpc, &space, invert)?, icc: space.icc() }
        }
    };
    if let Some(a) = alpha {
        if a.len() == px.rgba.len() / 4 {
            px.rgba.chunks_exact_mut(4).zip(a).for_each(|(p, a)| p[3] = a);
        }
    }
    image::encode(&px, ImageFormat::Png, 0.0).map(|png| Some((png, "png"))).map_err(|e| e.to_string())
}

/// `/Decode [1 0]` on a one-component image: dark and light swapped.
fn decode_inverted(doc: &Document, d: &Dictionary) -> bool {
    let Ok(arr) = d.get(b"Decode").and_then(|o| doc.dereference(o).map(|(_, o)| o)).and_then(Object::as_array) else { return false };
    arr.first().and_then(|v| v.as_float().ok()).is_some_and(|v| v > 0.5)
}

#[derive(Debug, Clone)]
enum Space {
    Gray,
    Rgb(Option<Vec<u8>>),
    Cmyk,
    /// Palette entries in the base space.
    Indexed(Box<Space>, Vec<u8>),
}

impl Space {
    fn components(&self) -> usize {
        match self {
            Self::Gray | Self::Indexed(..) => 1,
            Self::Rgb(_) => 3,
            Self::Cmyk => 4,
        }
    }

    fn base(&self) -> &Space {
        match self {
            Self::Indexed(b, _) => b,
            s => s,
        }
    }

    fn icc(&self) -> Option<Vec<u8>> {
        match self.base() {
            Self::Rgb(icc) => icc.clone(),
            _ => None,
        }
    }
}

fn color_space(doc: &Document, obj: Option<&Object>) -> std::result::Result<Space, String> {
    let Some(obj) = obj else { return Ok(Space::Rgb(None)) }; // JPEG/JPX bring their own
    let (_, obj) = doc.dereference(obj).map_err(|e| e.to_string())?;
    match obj {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" | b"G" => Ok(Space::Gray),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Ok(Space::Rgb(None)),
            b"DeviceCMYK" | b"CMYK" => Ok(Space::Cmyk),
            other => Err(format!("Farbraum {}", String::from_utf8_lossy(other))),
        },
        Object::Array(a) => {
            let kind = a.first().and_then(|o| o.as_name().ok()).ok_or("Farbraum")?;
            match kind {
                b"ICCBased" => {
                    let s = doc.dereference(a.get(1).ok_or("ICC")?).map_err(|e| e.to_string())?.1.as_stream().map_err(|e| e.to_string())?;
                    match int(doc, &s.dict, b"N") {
                        Some(1) => Ok(Space::Gray),
                        Some(3) => Ok(Space::Rgb(s.decompressed_content().ok())),
                        Some(4) => Ok(Space::Cmyk),
                        _ => Err("ICC".into()),
                    }
                }
                b"Indexed" | b"I" => {
                    let base = color_space(doc, a.get(1))?;
                    let lookup = match doc.dereference(a.get(3).ok_or("Palette")?).map_err(|e| e.to_string())?.1 {
                        Object::String(s, _) => s.clone(),
                        Object::Stream(s) => s.decompressed_content().map_err(|e| e.to_string())?,
                        _ => return Err("Palette".into()),
                    };
                    Ok(Space::Indexed(Box::new(base), lookup))
                }
                b"CalRGB" => Ok(Space::Rgb(None)),
                b"CalGray" => Ok(Space::Gray),
                other => Err(format!("Farbraum {}", String::from_utf8_lossy(other))),
            }
        }
        _ => Err("Farbraum".into()),
    }
}

/// Unpacks samples (1–16 bits) into straight RGBA.
fn to_rgba(raw: &[u8], w: u32, h: u32, bpc: u32, space: &Space, invert: bool) -> std::result::Result<Vec<u8>, String> {
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Err(format!("{bpc} Bit"));
    }
    let comps = space.components();
    let row_bits = w as usize * comps * bpc as usize;
    let row_bytes = row_bits.div_ceil(8);
    if raw.len() < row_bytes * h as usize {
        return Err("zu wenig Bilddaten".into());
    }
    let max = (1u32 << bpc.min(8)) - 1;
    let sample = |row: &[u8], i: usize| -> u32 {
        match bpc {
            8 => row[i] as u32,
            16 => row[i * 2] as u32, // high byte is enough for 8-bit output
            _ => {
                let bit = i * bpc as usize;
                (row[bit / 8] as u32 >> (8 - bpc as usize - bit % 8)) & max
            }
        }
    };
    let scale = |v: u32| (v * 255 / max) as u8;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let row = &raw[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..w as usize {
            let c = |k: usize| sample(row, x * comps + k);
            let rgb = match space {
                Space::Gray => {
                    let g = if invert { 255 - scale(c(0)) } else { scale(c(0)) };
                    [g, g, g]
                }
                Space::Rgb(_) => [scale(c(0)), scale(c(1)), scale(c(2))],
                Space::Cmyk => cmyk(scale(c(0)), scale(c(1)), scale(c(2)), scale(c(3))),
                Space::Indexed(base, lut) => {
                    let n = base.components();
                    let i = c(0) as usize * n;
                    let e = |k: usize| lut.get(i + k).copied().unwrap_or(0);
                    match **base {
                        Space::Gray => [e(0), e(0), e(0)],
                        Space::Cmyk => cmyk(e(0), e(1), e(2), e(3)),
                        _ => [e(0), e(1), e(2)],
                    }
                }
            };
            out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    Ok(out)
}

/// Plain CMYK → RGB (no colour management; good enough for pulled-out pictures).
fn cmyk(c: u8, m: u8, y: u8, k: u8) -> [u8; 3] {
    let f = |v: u8| ((255 - v as u32) * (255 - k as u32) / 255) as u8;
    [f(c), f(m), f(y)]
}

/// The soft mask (transparency) of an image, scaled to the image's size.
fn soft_mask(doc: &Document, d: &Dictionary, w: u32, h: u32) -> Option<Vec<u8>> {
    let s = doc.dereference(d.get(b"SMask").ok()?).ok()?.1.as_stream().ok()?;
    let (mw, mh) = (int(doc, &s.dict, b"Width")? as u32, int(doc, &s.dict, b"Height")? as u32);
    let bpc = int(doc, &s.dict, b"BitsPerComponent").unwrap_or(8) as u32;
    let raw = s.decompressed_content_with_limit((mw as usize * mh as usize * 2) + 4096).ok()?;
    let gray = to_rgba(&raw, mw, mh, bpc, &Space::Gray, false).ok()?;
    let mut a = Vec::with_capacity(w as usize * h as usize);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x * mw / w, y * mh / h);
            a.push(gray[((sy * mw + sx) * 4) as usize]);
        }
    }
    Some(a)
}

/// JPEG 2000, CMYK JPEGs and JPEGs with masks are decoded by macOS (ImageIO).
fn decode_with_imageio(data: &[u8], ext: &str) -> std::result::Result<Pixels, String> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("wolfgang-pdf-{}-{n}.{ext}", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let px = image::decode(&tmp, None, ColorTarget::KeepSource).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&tmp);
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_samples() {
        // 2×1 gray at 1 bit: black, white
        assert_eq!(to_rgba(&[0b0100_0000], 2, 1, 1, &Space::Gray, false).unwrap(), [0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(to_rgba(&[0b0100_0000], 2, 1, 1, &Space::Gray, true).unwrap()[..4], [255, 255, 255, 255]);
        // Indexed with an RGB palette
        let pal = Space::Indexed(Box::new(Space::Rgb(None)), vec![0, 0, 0, 200, 10, 20]);
        assert_eq!(to_rgba(&[1, 0], 2, 1, 8, &pal, false).unwrap(), [200, 10, 20, 255, 0, 0, 0, 255]);
        assert_eq!(to_rgba(&[0, 0, 0, 255], 1, 1, 8, &Space::Cmyk, false).unwrap(), [0, 0, 0, 255]);
        assert_eq!(cmyk(0, 0, 0, 0), [255, 255, 255]);
        assert!(to_rgba(&[1], 4, 4, 8, &Space::Gray, false).is_err(), "too little data");
    }
}
