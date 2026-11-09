//! The `.sprite` text format (see `assets/pixel/src/wolfgang.sprite`) and
//! rendering it into a sprite sheet.

use serde::Serialize;
use std::collections::BTreeMap;
use wolfgang_core::image::Pixels;

#[derive(Debug, Clone)]
pub struct Frame {
    pub name: String,
    pub w: u32,
    pub h: u32,
    pub slice: Option<u32>,
    pub rows: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SpriteFile {
    pub origin: Option<(i32, i32)>,
    pub palette: BTreeMap<char, [u8; 3]>,
    pub frames: Vec<Frame>,
    /// name → (frame, milliseconds); 0 ms = hold.
    pub anims: BTreeMap<String, Vec<(String, u32)>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SheetFrame {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice: Option<u32>,
}

pub fn parse(text: &str) -> Result<SpriteFile, String> {
    let mut size = None;
    let mut f = SpriteFile { origin: None, palette: BTreeMap::new(), frames: Vec::new(), anims: BTreeMap::new() };
    let mut current: Option<Frame> = None;
    let err = |n: usize, msg: String| format!("line {}: {msg}", n + 1);

    for (n, raw) in text.lines().enumerate() {
        // Inside a frame, rows are taken literally until it is complete.
        if let Some(fr) = current.as_mut() {
            if (fr.rows.len() as u32) < fr.h {
                if raw.chars().count() as u32 != fr.w {
                    return Err(err(n, format!("frame '{}' row {} has {} characters, expected {}", fr.name, fr.rows.len() + 1, raw.chars().count(), fr.w)));
                }
                fr.rows.push(raw.to_owned());
                continue;
            }
            f.frames.push(current.take().unwrap());
        }
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| parts.get(i).and_then(|s| s.parse::<i64>().ok()).ok_or_else(|| err(n, format!("expected a number in '{line}'")));
        match parts[0] {
            "size" => size = Some((num(1)? as u32, num(2)? as u32)),
            "origin" => f.origin = Some((num(1)? as i32, num(2)? as i32)),
            "color" => {
                let ch = parts.get(1).and_then(|s| s.chars().next()).ok_or_else(|| err(n, "color needs a character".into()))?;
                let hex = parts.get(2).ok_or_else(|| err(n, "color needs RRGGBB".into()))?;
                let v = u32::from_str_radix(hex, 16).map_err(|_| err(n, format!("bad colour '{hex}'")))?;
                if ch == '.' {
                    return Err(err(n, "'.' is reserved for transparent".into()));
                }
                f.palette.insert(ch, [(v >> 16) as u8, (v >> 8) as u8, v as u8]);
            }
            "frame" => {
                let name = parts.get(1).ok_or_else(|| err(n, "frame needs a name".into()))?.to_string();
                let (w, h) = if parts.len() >= 4 { (num(2)? as u32, num(3)? as u32) } else { size.ok_or_else(|| err(n, "no size given".into()))? };
                let slice = match parts.iter().position(|p| *p == "slice") {
                    Some(i) => Some(num(i + 1)? as u32),
                    None => None,
                };
                current = Some(Frame { name, w, h, slice, rows: Vec::new() });
            }
            "anim" => {
                let name = parts.get(1).ok_or_else(|| err(n, "anim needs a name".into()))?.to_string();
                let steps = parts[2..]
                    .iter()
                    .map(|s| {
                        let (fr, ms) = s.split_once(':').ok_or_else(|| err(n, format!("'{s}' should be frame:ms")))?;
                        Ok((fr.to_owned(), ms.parse().map_err(|_| err(n, format!("bad duration in '{s}'")))?))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                f.anims.insert(name, steps);
            }
            other => return Err(err(n, format!("unknown keyword '{other}'"))),
        }
    }
    if let Some(fr) = current.take() {
        if (fr.rows.len() as u32) < fr.h {
            return Err(format!("frame '{}' has only {} of {} rows", fr.name, fr.rows.len(), fr.h));
        }
        f.frames.push(fr);
    }
    // Every pixel uses a known colour; every animation step a known frame.
    for fr in &f.frames {
        for (y, row) in fr.rows.iter().enumerate() {
            if let Some(c) = row.chars().find(|c| *c != '.' && !f.palette.contains_key(c)) {
                return Err(format!("frame '{}' row {}: colour '{c}' is not in the palette", fr.name, y + 1));
            }
        }
    }
    for (name, steps) in &f.anims {
        for (fr, _) in steps {
            if !f.frames.iter().any(|x| &x.name == fr) {
                return Err(format!("anim '{name}' uses unknown frame '{fr}'"));
            }
        }
    }
    Ok(f)
}

impl Frame {
    pub fn rgba(&self, palette: &BTreeMap<char, [u8; 3]>) -> Vec<u8> {
        self.rows
            .iter()
            .flat_map(|r| r.chars())
            .flat_map(|c| match palette.get(&c) {
                Some([r, g, b]) => [*r, *g, *b, 255],
                None => [0, 0, 0, 0],
            })
            .collect()
    }
}

/// All frames side by side (1 px gap so filtering never bleeds).
pub fn sheet(f: &SpriteFile) -> (Pixels, BTreeMap<String, SheetFrame>) {
    let w: u32 = f.frames.iter().map(|fr| fr.w + 1).sum();
    let h = f.frames.iter().map(|fr| fr.h).max().unwrap_or(1);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut map = BTreeMap::new();
    let mut x0 = 0;
    for fr in &f.frames {
        let px = fr.rgba(&f.palette);
        for y in 0..fr.h {
            let src = &px[(y * fr.w * 4) as usize..((y + 1) * fr.w * 4) as usize];
            let dst = ((y * w + x0) * 4) as usize;
            rgba[dst..dst + src.len()].copy_from_slice(src);
        }
        map.insert(fr.name.clone(), SheetFrame { x: x0, y: 0, w: fr.w, h: fr.h, slice: fr.slice });
        x0 += fr.w + 1;
    }
    (Pixels { width: w, height: h, rgba, icc: None }, map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates() {
        let ok = "size 3 2\ncolor X ff0000  # red\nframe a\nX.X\n.X.\nframe b 2 1 slice 1\nXX\nanim blink a:100 b:0\n";
        let f = parse(ok).unwrap();
        assert_eq!(f.frames.len(), 2);
        assert_eq!(f.frames[1].slice, Some(1));
        assert_eq!(f.anims["blink"], vec![("a".to_string(), 100), ("b".to_string(), 0)]);
        let (img, map) = sheet(&f);
        assert_eq!((img.width, img.height), (3 + 1 + 2 + 1, 2));
        assert_eq!(map["b"].x, 4);
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);

        assert!(parse("size 3 1\ncolor X ff0000\nframe a\nXX\n").unwrap_err().contains("2 characters"));
        assert!(parse("size 1 1\nframe a\nZ\n").unwrap_err().contains("'Z' is not in the palette"));
        assert!(parse("size 1 1\ncolor X 000000\nframe a\nX\nanim x nope:10\n").unwrap_err().contains("unknown frame 'nope'"));
    }

    #[test]
    fn repo_sources_are_valid() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/pixel/src");
        for name in ["wolfgang.sprite", "ui.sprite", "wilma.sprite"] {
            let text = std::fs::read_to_string(root.join(name)).unwrap();
            parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
