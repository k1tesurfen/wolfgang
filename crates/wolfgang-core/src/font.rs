//! Fonts → WOFF2, converted 1:1 (no subsetting).
//!
//! Encoding uses Google's reference woff2 library. Because WOFF2 rewrites the
//! `glyf`/`loca` tables, every result is decoded again and compared with the
//! original; if anything differs we fall back to encoding without that transform.

use crate::error::{Error, Result};
use crate::naming;
use read_fonts::tables::glyf::Glyph;
use read_fonts::types::{GlyphId, Tag};
use read_fonts::{FontRef, TableProvider};
use skrifa::string::StringId;
use skrifa::MetadataProvider;

/// Brotli quality for WOFF2 (max).
const BROTLI_QUALITY: usize = 11;

/// How the WOFF2 was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Woff2Mode {
    /// Normal WOFF2 with the glyph table transform.
    Transformed,
    /// The transformed result didn't match the original, so the glyph tables
    /// were stored untransformed (slightly bigger, byte-faithful).
    Untransformed,
}

pub struct Woff2 {
    pub data: Vec<u8>,
    pub mode: Woff2Mode,
}

/// Turns TTF/OTF/WOFF data into a plain sfnt (TTF/OTF) font.
pub fn to_sfnt(data: &[u8]) -> Result<Vec<u8>> {
    match data.get(..4) {
        Some(b"wOFF") => woff1_to_sfnt(data),
        Some(b"wOF2") => woofwoof::decompress(data).ok_or_else(|| Error::Font("WOFF2 lässt sich nicht entpacken".into())),
        Some(b"ttcf") => Err(Error::Font("Schriftsammlungen (.ttc) kann ich noch nicht".into())),
        Some(&[0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") => {
            FontRef::new(data).map_err(|e| Error::Font(format!("Datei ist beschädigt ({e})")))?;
            Ok(data.to_vec())
        }
        _ => Err(Error::Font("Das ist keine Schriftdatei, die ich kenne".into())),
    }
}

/// Encodes to WOFF2 and proves the result decodes back to the same font.
pub fn to_woff2(sfnt: &[u8]) -> Result<Woff2> {
    let original = FontRef::new(sfnt).map_err(|e| Error::Font(format!("Datei ist beschädigt ({e})")))?;
    for (transform, mode) in [(true, Woff2Mode::Transformed), (false, Woff2Mode::Untransformed)] {
        let Some(data) = woofwoof::compress(sfnt, "", BROTLI_QUALITY, transform) else { continue };
        if let Some(decoded) = woofwoof::decompress(&data) {
            if let Ok(decoded) = FontRef::new(&decoded) {
                if same_font(&original, &decoded).is_ok() {
                    return Ok(Woff2 { data, mode });
                }
            }
        }
    }
    Err(Error::Font("WOFF2 wäre nicht identisch mit dem Original geworden, deshalb hab ich's gelassen".into()))
}

/// Compares two fonts table by table. `glyf` is compared glyph by glyph
/// (WOFF2 may re-encode flags/coordinates), `loca` follows from that, and the
/// `head` fields WOFF2 must change (checksum adjustment, flag bit 11) are masked.
/// A dropped `DSIG` is accepted: the signature is invalid after any repacking
/// and browsers ignore it.
pub fn same_font(a: &FontRef, b: &FontRef) -> std::result::Result<(), String> {
    let tags = |f: &FontRef| f.table_directory().table_records().iter().map(|r| r.tag()).collect::<std::collections::BTreeSet<Tag>>();
    let (ta, tb) = (tags(a), tags(b));
    let dsig = Tag::new(b"DSIG");
    let missing: Vec<_> = ta.symmetric_difference(&tb).filter(|t| **t != dsig).collect();
    if !missing.is_empty() {
        return Err(format!("Tabellen unterschiedlich: {missing:?}"));
    }
    for tag in ta.intersection(&tb) {
        let (da, db) = (a.table_data(*tag).unwrap(), b.table_data(*tag).unwrap());
        let ok = match &tag.to_be_bytes() {
            b"glyf" | b"loca" => true,
            b"head" => masked_head(da.as_bytes()) == masked_head(db.as_bytes()),
            b"DSIG" => true,
            _ => da.as_bytes() == db.as_bytes(),
        };
        if !ok {
            return Err(format!("Tabelle {tag} unterschiedlich"));
        }
    }
    if ta.contains(&Tag::new(b"glyf")) {
        same_glyphs(a, b)?;
    }
    Ok(())
}

fn masked_head(head: &[u8]) -> Vec<u8> {
    let mut h = head.to_vec();
    if h.len() >= 18 {
        h[8..12].fill(0); // checkSumAdjustment
        h[16] &= !0x08; // flags bit 11: "lossless font data transform applied"
    }
    h
}

fn same_glyphs(a: &FontRef, b: &FontRef) -> std::result::Result<(), String> {
    let err = |e: read_fonts::ReadError| e.to_string();
    let n = a.maxp().map_err(err)?.num_glyphs();
    if b.maxp().map_err(err)?.num_glyphs() != n {
        return Err("Glyphenanzahl unterschiedlich".into());
    }
    let (la, ga) = (a.loca(None).map_err(err)?, a.glyf().map_err(err)?);
    let (lb, gb) = (b.loca(None).map_err(err)?, b.glyf().map_err(err)?);
    for gid in 0..n {
        let id = GlyphId::new(gid as u32);
        let x = la.get_glyf(id, &ga).map_err(err)?;
        let y = lb.get_glyf(id, &gb).map_err(err)?;
        let same = match (&x, &y) {
            (None, None) => true,
            (Some(Glyph::Simple(x)), Some(Glyph::Simple(y))) => {
                (x.x_min(), x.y_min(), x.x_max(), x.y_max()) == (y.x_min(), y.y_min(), y.x_max(), y.y_max())
                    && x.end_pts_of_contours() == y.end_pts_of_contours()
                    && x.instructions() == y.instructions()
                    && x.points().eq(y.points())
            }
            (Some(Glyph::Composite(x)), Some(Glyph::Composite(y))) => {
                (x.x_min(), x.y_min(), x.x_max(), x.y_max()) == (y.x_min(), y.y_min(), y.x_max(), y.y_max())
                    && x.components().eq(y.components())
                    && x.instructions() == y.instructions()
            }
            _ => false,
        };
        if !same {
            return Err(format!("Glyphe {gid} unterschiedlich"));
        }
    }
    Ok(())
}

/// Web-friendly file stem from the font's own metadata:
/// `Inter Bold Italic.otf` → `inter-700-italic`, variable fonts → `inter-variable`.
pub fn web_name(sfnt: &[u8]) -> Option<String> {
    let font = skrifa::FontRef::new(sfnt).ok()?;
    let family = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| font.localized_strings(id).english_or_first())
        .map(|s| s.chars().collect::<String>())?;
    let mut words = naming::words(&family);
    if words.is_empty() {
        return None;
    }
    let os2 = font.os2().ok();
    let italic = os2.as_ref().is_some_and(|o| o.fs_selection().bits() & 1 != 0)
        || font.head().is_ok_and(|h| h.mac_style().bits() & 2 != 0);
    if font.table_data(Tag::new(b"fvar")).is_some() {
        if !words.last().is_some_and(|w| w.eq_ignore_ascii_case("variable")) {
            words.push("variable".into()); // "Inter Variable" already says so
        }
    } else {
        words.push(os2.map(|o| o.us_weight_class()).unwrap_or(400).to_string());
    }
    if italic {
        words.push("italic".into());
    }
    Some(naming::join(&words, naming::Separator::Dash))
}

/// WOFF 1.0: a table directory with optionally zlib-compressed tables.
fn woff1_to_sfnt(data: &[u8]) -> Result<Vec<u8>> {
    let bad = || Error::Font("WOFF-Datei ist beschädigt".into());
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or_else(bad);
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(bad);

    let flavor = u32_at(4)?;
    let num_tables = u16_at(12)? as usize;
    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let e = 44 + i * 20;
        let (tag, offset, comp_len, orig_len, checksum) = (u32_at(e)?, u32_at(e + 4)? as usize, u32_at(e + 8)? as usize, u32_at(e + 12)? as usize, u32_at(e + 16)?);
        let raw = data.get(offset..offset + comp_len).ok_or_else(bad)?;
        let table = if comp_len < orig_len {
            let mut out = Vec::with_capacity(orig_len);
            std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(raw), &mut out).map_err(|_| bad())?;
            out
        } else {
            raw.to_vec()
        };
        if table.len() != orig_len {
            return Err(bad());
        }
        tables.push((tag, checksum, table));
    }
    tables.sort_by_key(|t| t.0);

    let mut out = Vec::new();
    let pow2 = if num_tables == 0 { 0 } else { 1usize << (usize::BITS - 1 - num_tables.leading_zeros()) };
    let search_range = (pow2 * 16) as u16;
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&(pow2.max(1).trailing_zeros() as u16).to_be_bytes());
    out.extend_from_slice(&((num_tables * 16) as u16 - search_range).to_be_bytes());
    let mut offset = 12 + num_tables * 16;
    for (tag, checksum, table) in &tables {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&checksum.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        offset += table.len().next_multiple_of(4);
    }
    for (_, _, table) in &tables {
        out.extend_from_slice(table);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    pub const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
    /// A CFF-flavoured (`OTTO`) font that ships with macOS.
    pub const CFF_OTF: &str = "/System/Library/Fonts/Supplemental/NotoSansJavanese-Regular.otf";

    pub fn system_font(path: &str) -> Option<Vec<u8>> {
        std::fs::read(Path::new(path)).ok()
    }

    /// Builds an (uncompressed or compressed) WOFF 1.0 file from an sfnt.
    pub fn make_woff1(sfnt: &[u8], compress: bool) -> Vec<u8> {
        let font = FontRef::new(sfnt).unwrap();
        let records = font.table_directory().table_records();
        let mut body = Vec::new();
        let mut dir = Vec::new();
        let header_len = 44 + records.len() * 20;
        for r in records {
            let data = font.table_data(r.tag()).unwrap().as_bytes().to_vec();
            let stored = if compress {
                use std::io::Write;
                let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
                z.write_all(&data).unwrap();
                let c = z.finish().unwrap();
                if c.len() < data.len() { c } else { data.clone() }
            } else {
                data.clone()
            };
            dir.extend_from_slice(&r.tag().to_be_bytes());
            dir.extend_from_slice(&((header_len + body.len()) as u32).to_be_bytes());
            dir.extend_from_slice(&(stored.len() as u32).to_be_bytes());
            dir.extend_from_slice(&(data.len() as u32).to_be_bytes());
            dir.extend_from_slice(&r.checksum().to_be_bytes());
            body.extend_from_slice(&stored);
            body.resize(body.len().next_multiple_of(4), 0);
        }
        let mut out = b"wOFF".to_vec();
        out.extend_from_slice(&sfnt[..4]);
        out.extend_from_slice(&((header_len + body.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(records.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 2 + 4 + 2 + 2 + 4 + 4 + 4 + 4 + 4]); // reserved .. privLength
        out.extend_from_slice(&dir);
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn truetype_roundtrips_through_woff2() {
        let Some(ttf) = system_font(ARIAL) else { return };
        let w = to_woff2(&to_sfnt(&ttf).unwrap()).unwrap();
        assert_eq!(&w.data[..4], b"wOF2");
        assert_eq!(w.mode, Woff2Mode::Transformed);
        assert!(w.data.len() < ttf.len() / 2, "{} vs {}", w.data.len(), ttf.len());
        assert_eq!(web_name(&ttf).as_deref(), Some("arial-400"));
    }

    #[test]
    fn cff_roundtrips_through_woff2() {
        let Some(otf) = system_font(CFF_OTF) else { return };
        assert_eq!(&otf[..4], b"OTTO");
        let w = to_woff2(&otf).unwrap();
        assert_eq!(w.mode, Woff2Mode::Transformed);
        let back = woofwoof::decompress(&w.data).unwrap();
        same_font(&FontRef::new(&otf).unwrap(), &FontRef::new(&back).unwrap()).unwrap();
    }

    #[test]
    fn woff1_is_unpacked_to_identical_tables() {
        let Some(ttf) = system_font(ARIAL) else { return };
        for compress in [false, true] {
            let sfnt = to_sfnt(&make_woff1(&ttf, compress)).unwrap();
            same_font(&FontRef::new(&ttf).unwrap(), &FontRef::new(&sfnt).unwrap()).unwrap();
        }
    }

    #[test]
    fn detects_modified_glyphs() {
        let Some(ttf) = system_font(ARIAL) else { return };
        let font = FontRef::new(&ttf).unwrap();
        let glyf = font.table_directory().table_records().iter().find(|r| r.tag() == Tag::new(b"glyf")).unwrap();
        // Flip a byte deep inside the glyph data (in some glyph's coordinates).
        let mut broken = ttf.clone();
        let pos = glyf.offset() as usize + glyf.length() as usize / 2;
        broken[pos] ^= 0x55;
        assert!(same_font(&font, &FontRef::new(&broken).unwrap()).is_err());
    }

    #[test]
    fn rejects_non_fonts_and_collections() {
        assert!(to_sfnt(b"hello world").is_err());
        assert!(matches!(to_sfnt(b"ttcf\0\0\0\0"), Err(Error::Font(m)) if m.contains(".ttc")));
    }
}

