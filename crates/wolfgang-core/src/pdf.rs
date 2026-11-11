//! PDFs, two ways:
//! - pulling the pictures out, as they are: JPEGs stay JPEGs (copied byte for
//!   byte), everything else becomes PNG. Nothing is rendered.
//! - making the whole PDF smaller for downloads: oversized pictures are scaled
//!   down and packed as JPEG, editor leftovers (Illustrator's private data, page
//!   thumbnails) go. Text, vectors and fonts stay untouched.

use crate::error::{io_err, Error, Result};
use crate::image::{self, ColorTarget, Pixels};
use crate::settings::ImageFormat;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn load_err(name: &str) -> impl FnOnce(lopdf::Error) -> Error + '_ {
    move |e| match e {
        lopdf::Error::Decryption(_) | lopdf::Error::InvalidPassword => Error::Pdf(format!("{name} ist mit einem Passwort geschützt")),
        other => Error::Pdf(format!("Kann {name} nicht als PDF lesen ({other})")),
    }
}

pub fn extract(path: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<Extraction> {
    let name = file_name(path);
    let doc = Document::load(path).map_err(load_err(&name))?;
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
            decode_with_imageio(&stream.content, "jpg", ColorTarget::KeepSource)?
        }
        Some(b"JPXDecode") if single_filter => decode_with_imageio(&stream.content, "jp2", ColorTarget::KeepSource)?,
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
fn decode_with_imageio(data: &[u8], ext: &str, color: ColorTarget) -> std::result::Result<Pixels, String> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("wolfgang-pdf-{}-{n}.{ext}", std::process::id()));
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    let px = image::decode(&tmp, None, color).map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&tmp);
    px
}

// ── Smaller for downloads ────────────────────────────────────────────────────

/// Longest side of a picture in a smaller PDF: a full A4 page at about 170 dpi.
const SHRINK_SIDE: u32 = 2000;
/// JPEG quality of re-packed pictures (mozjpeg).
const SHRINK_QUALITY: f32 = 75.0;
/// A picture is only replaced if that saves at least this much …
const PICTURE_SAVING: f64 = 0.1;
/// … and the PDF only if the whole file shrinks by this much.
const FILE_SAVING: f64 = 0.05;

pub struct Shrunk {
    pub data: Vec<u8>,
    /// `false`: not worth it, `data` is the original.
    pub smaller: bool,
}

/// The PDF, smaller. Every page is still there (checked by reading the result back).
pub fn shrink(path: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<Shrunk> {
    let name = file_name(path);
    let original = std::fs::read(path).map_err(io_err(path))?;
    let mut doc = Document::load_mem(&original).map_err(load_err(&name))?;
    if doc.is_encrypted() {
        return Err(Error::Pdf(format!("{name} ist geschützt – da fass ich nichts an")));
    }
    if signed(&doc) {
        return Err(Error::Pdf(format!("{name} ist digital unterschrieben – kleiner gemacht wäre die Unterschrift ungültig")));
    }
    let pages = doc.get_pages().len();

    // Transparency masks stay as they are; they belong to their picture.
    let masks: HashSet<ObjectId> = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .flat_map(|s| [b"SMask".as_slice(), b"Mask"].into_iter().filter_map(|k| s.dict.get(k).ok()?.as_reference().ok()))
        .collect();
    let pictures: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(id, o)| !masks.contains(id) && o.as_stream().is_ok_and(|s| s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image")))
        .map(|(id, _)| *id)
        .collect();
    let done = AtomicUsize::new(0);
    let replaced = pictures
        .par_iter()
        .map(|id| {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            // A picture Wolfgang can't read is simply left as it is.
            let smaller = doc.get_object(*id).and_then(Object::as_stream).ok().and_then(|s| smaller_picture(&doc, s).ok().flatten());
            progress(0.9 * (done.fetch_add(1, Ordering::Relaxed) + 1) as f64 / pictures.len() as f64);
            Ok(smaller.map(|s| (*id, s)))
        })
        .collect::<Result<Vec<_>>>()?;
    for (id, s) in replaced.into_iter().flatten() {
        doc.objects.insert(id, Object::Stream(s));
    }
    strip_leftovers(&mut doc);
    doc.prune_objects();
    doc.compress();
    let mut out = Vec::new();
    doc.save_modern(&mut out).map_err(|e| Error::Pdf(format!("Beim Speichern von {name} ging was schief ({e})")))?;
    let check = Document::load_mem(&out).map_err(|e| Error::Pdf(format!("Das kleinere {name} wäre kaputt ({e}) – lass ich lieber")))?;
    if check.get_pages().len() != pages {
        return Err(Error::Pdf(format!("Beim kleineren {name} fehlen Seiten – lass ich lieber")));
    }
    progress(1.0);
    if out.len() as f64 > original.len() as f64 * (1.0 - FILE_SAVING) {
        return Ok(Shrunk { data: original, smaller: false });
    }
    Ok(Shrunk { data: out, smaller: true })
}

/// No `/Decode`, or one that changes nothing (`[0 1 0 1 …]`, which layout programs like to write).
fn plain_decode(doc: &Document, d: &Dictionary) -> bool {
    let Ok(obj) = d.get(b"Decode") else { return true };
    let Ok(arr) = doc.dereference(obj).map(|(_, o)| o).and_then(Object::as_array) else { return false };
    arr.iter().enumerate().all(|(i, v)| v.as_float().is_ok_and(|v| v == (i % 2) as f32))
}

/// A digital signature breaks as soon as anything in the file changes.
fn signed(doc: &Document) -> bool {
    let form = doc.catalog().ok().and_then(|c| c.get(b"AcroForm").ok()).and_then(|f| doc.dereference(f).ok()).and_then(|(_, f)| f.as_dict().ok());
    form.and_then(|f| int(doc, f, b"SigFlags")).is_some_and(|flags| flags & 1 != 0)
}

/// The picture scaled down to [`SHRINK_SIDE`] and packed as JPEG; `None` if
/// that's not clearly smaller or the picture is better left alone (line art,
/// palettes, masks, unusual encodings).
fn smaller_picture(doc: &Document, stream: &Stream) -> std::result::Result<Option<Stream>, String> {
    let d = &stream.dict;
    if d.get(b"ImageMask").and_then(Object::as_bool).unwrap_or(false) || d.has(b"Mask") {
        return Ok(None);
    }
    let (w, h) = (int(doc, d, b"Width").ok_or("ohne Breite")?, int(doc, d, b"Height").ok_or("ohne Höhe")?);
    if w < MIN_SIDE || h < MIN_SIDE || w * h > MAX_PIXELS {
        return Ok(None);
    }
    let space = color_space(doc, d.get(b"ColorSpace").ok())?;
    if matches!(space, Space::Indexed(..)) {
        return Ok(None); // logos and diagrams: JPEG would smear them
    }
    let single_filter = stream.filters().map(|f| f.len() == 1).unwrap_or(false);
    let decode_array = !plain_decode(doc, d);
    // RGB pictures keep their colour space (and its profile); everything else becomes sRGB.
    let mut keep_space = matches!(space, Space::Rgb(_)) && d.has(b"ColorSpace");
    let px = match last_filter(stream).as_deref() {
        Some(b"DCTDecode") if single_filter && !decode_array => {
            decode_with_imageio(&stream.content, "jpg", if keep_space { ColorTarget::KeepSource } else { ColorTarget::Srgb })?
        }
        Some(b"JPXDecode") if single_filter && !decode_array => {
            keep_space = false; // JPEG 2000 brings its own colours
            decode_with_imageio(&stream.content, "jp2", ColorTarget::Srgb)?
        }
        Some(b"DCTDecode" | b"JPXDecode" | b"JBIG2Decode" | b"CCITTFaxDecode") => return Ok(None),
        _ => {
            if int(doc, d, b"BitsPerComponent").unwrap_or(8) != 8 || decode_array {
                return Ok(None); // 1-bit scans and the like stay sharp
            }
            let raw = stream.decompressed_content_with_limit((w * h * 4 + 4096) as usize).map_err(|e| e.to_string())?;
            Pixels { width: w as u32, height: h as u32, rgba: to_rgba(&raw, w as u32, h as u32, 8, &space, false)?, icc: None }
        }
    };
    let (width, height, rgba) = if px.width.max(px.height) > SHRINK_SIDE {
        image::resize_premultiplied(px.width, px.height, px.rgba, SHRINK_SIDE).map_err(|e| e.to_string())?
    } else {
        (px.width, px.height, px.rgba)
    };
    let jpeg = image::encode(&Pixels { width, height, rgba, icc: None }, ImageFormat::Jpg, SHRINK_QUALITY).map_err(|e| e.to_string())?;
    if jpeg.len() as f64 > stream.content.len() as f64 * (1.0 - PICTURE_SAVING) {
        return Ok(None);
    }
    let mut dict = d.clone();
    for key in [b"DecodeParms".as_slice(), b"Decode", b"SMaskInData", b"Length", b"Metadata"] {
        dict.remove(key);
    }
    dict.set("Filter", "DCTDecode");
    dict.set("Width", width as i64);
    dict.set("Height", height as i64);
    dict.set("BitsPerComponent", 8);
    if !keep_space {
        dict.set("ColorSpace", "DeviceRGB");
    }
    // A soft mask keeps working at another size: both are stretched over the same area.
    let mut s = Stream::new(dict, jpeg);
    s.allows_compression = false;
    Ok(Some(s))
}

/// Data no viewer needs: Illustrator's and InDesign's private copies of the
/// document (`PieceInfo`, often most of the file), page thumbnails and XMP on pictures.
fn strip_leftovers(doc: &mut Document) {
    for obj in doc.objects.values_mut() {
        let dict = match obj {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &mut s.dict,
            _ => continue,
        };
        dict.remove(b"PieceInfo");
        if dict.get(b"Type").and_then(Object::as_name).is_ok_and(|t| t == b"Page") {
            dict.remove(b"Thumb");
        }
        if dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|t| t == b"Image" || t == b"Form") {
            dict.remove(b"Metadata");
        }
    }
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

    #[test]
    fn a_decode_that_changes_nothing_is_ignored() {
        let doc = Document::new();
        let with = |arr: Vec<Object>| lopdf::dictionary! { "Decode" => arr };
        assert!(plain_decode(&doc, &Dictionary::new()));
        assert!(plain_decode(&doc, &with(vec![0.into(), 1.into(), 0.into(), 1.into()])));
        assert!(plain_decode(&doc, &with(vec![Object::Real(0.0), Object::Real(1.0)])));
        assert!(!plain_decode(&doc, &with(vec![1.into(), 0.into(), 1.into(), 0.into()])), "inverted CMYK");
    }
}
