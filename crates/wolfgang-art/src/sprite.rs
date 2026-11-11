//! The `.sprite` text format (see `assets/pixel/characters/wolfgang/desk.sprite`) and
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

/// Characters handed out to colours that appear in an edited sheet for the first time.
const FREE_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!$%&*+-/:;<=>?@^_~";

/// The result of writing an edited sheet back into a `.sprite` text.
#[derive(Debug)]
pub struct Applied {
    pub text: String,
    pub changed: usize,
    pub new_colors: Vec<(char, [u8; 3])>,
    pub warnings: Vec<String>,
}

/// Writes an edited sheet (laid out exactly like [`sheet`]) back into the `.sprite`
/// text: frame rows are replaced, colours seen for the first time get a free
/// character. Comments, palette, frame names and animations stay as they are.
pub fn apply_sheet(text: &str, img: &Pixels) -> Result<Applied, String> {
    let f = parse(text)?;
    let (expected, map) = sheet(&f);
    if (img.width, img.height) != (expected.width, expected.height) {
        return Err(format!("the PNG is {}×{}, the sprite sheet is {}×{}: frames can't be found", img.width, img.height, expected.width, expected.height));
    }
    let mut by_color: BTreeMap<[u8; 3], char> = BTreeMap::new();
    for (ch, c) in f.palette.iter().rev() {
        by_color.insert(*c, *ch);
    }
    let mut free = FREE_CHARS.chars().filter(|c| !f.palette.contains_key(c));
    let mut new_colors = Vec::new();
    let (mut changed, mut half, mut in_gaps) = (0, 0, 0);
    let mut inside = vec![false; (img.width * img.height) as usize];
    let mut rows: Vec<Vec<String>> = Vec::new();
    for fr in &f.frames {
        let at = &map[&fr.name];
        let mut out = Vec::with_capacity(fr.h as usize);
        for y in 0..fr.h {
            let old: Vec<char> = fr.rows[y as usize].chars().collect();
            let mut row = String::with_capacity(fr.w as usize);
            for x in 0..fr.w {
                let o = (y * img.width + at.x + x) as usize;
                inside[o] = true;
                let p = &img.rgba[o * 4..o * 4 + 4];
                if p[3] != 0 && p[3] != 255 {
                    half += 1;
                }
                let ch = if p[3] < 128 {
                    '.'
                } else {
                    let c = [p[0], p[1], p[2]];
                    // Two letters may share a colour (lanyard red = skin shadow): an unchanged pixel keeps its letter.
                    match by_color.get(&c) {
                        _ if f.palette.get(&old[x as usize]) == Some(&c) => old[x as usize],
                        Some(ch) => *ch,
                        None => {
                            let ch = free.next().ok_or("too many new colours: no free characters left")?;
                            by_color.insert(c, ch);
                            new_colors.push((ch, c));
                            ch
                        }
                    }
                };
                changed += usize::from(ch != old[x as usize]);
                row.push(ch);
            }
            out.push(row);
        }
        rows.push(out);
    }
    for (o, inside) in inside.iter().enumerate() {
        in_gaps += usize::from(!inside && img.rgba[o * 4 + 3] >= 128);
    }
    let mut warnings = Vec::new();
    if half > 0 {
        warnings.push(format!("{half} half-transparent pixels (smoothing brush?): taken as opaque or transparent"));
    }
    if in_gaps > 0 {
        warnings.push(format!("{in_gaps} pixels painted between frames: ignored"));
    }

    // Same walk as `parse`: rows are taken literally, the rest is copied. New colours go
    // after the last `color` line, or (no palette yet) before the first frame or anim.
    let mut lines = Vec::new();
    let (mut skip, mut frame, mut after_colors, mut first_block) = (0u32, 0, None, None);
    for raw in text.lines() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        let kw = keyword(raw);
        if first_block.is_none() && matches!(kw, Some("frame" | "anim")) {
            first_block = Some(lines.len());
        }
        lines.push(raw.to_owned());
        match kw {
            Some("color") => after_colors = Some(lines.len()),
            Some("frame") => {
                lines.extend(rows[frame].iter().cloned());
                skip = f.frames[frame].h;
                frame += 1;
            }
            _ => {}
        }
    }
    let at = after_colors.or(first_block).unwrap_or(lines.len());
    lines.splice(at..at, new_colors.iter().map(|(ch, [r, g, b])| format!("color {ch} {r:02x}{g:02x}{b:02x}  # new (painted)")));
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    parse(&out).map_err(|e| format!("internal: the written sprite doesn't parse: {e}"))?;
    Ok(Applied { text: out, changed, new_colors, warnings })
}

/// The first word of a line outside a frame (`size`, `color`, `frame`, …), comments ignored.
fn keyword(line: &str) -> Option<&str> {
    line.split('#').next().unwrap_or("").split_whitespace().next()
}

/// A sprite's layout (size, origin, frames, animations) with no palette and empty
/// frames: what a sheet painted from scratch is read into, with [`apply_sheet`].
pub fn blank(text: &str) -> Result<String, String> {
    let f = parse(text)?;
    let mut lines = Vec::new();
    let (mut skip, mut frame) = (0u32, 0);
    for raw in text.lines() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        let kw = keyword(raw);
        if kw == Some("color") {
            continue;
        }
        lines.push(raw.to_owned());
        if kw == Some("frame") {
            let fr = &f.frames[frame];
            lines.extend((0..fr.h).map(|_| ".".repeat(fr.w as usize)));
            skip = fr.h;
            frame += 1;
        }
    }
    Ok(lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edited_sheet_goes_back_into_the_text() {
        let text = "# test\nsize 2 1\ncolor X ff0000  # red\nframe a\nX.\nframe b\n.X\nanim x a:10 b:10\n";
        let (mut img, _) = sheet(&parse(text).unwrap());
        // Untouched: the text comes back byte for byte.
        let same = apply_sheet(text, &img).unwrap();
        assert_eq!((same.text.as_str(), same.changed), (text, 0));
        // Frame b, pixel 0 painted in a new colour; frame a, pixel 0 erased.
        img.rgba[12..16].copy_from_slice(&[0, 0, 255, 255]);
        img.rgba[0..4].copy_from_slice(&[0, 0, 0, 0]);
        let a = apply_sheet(text, &img).unwrap();
        assert_eq!(a.changed, 2);
        assert_eq!(a.new_colors, vec![('A', [0, 0, 255])]);
        assert_eq!(a.text, "# test\nsize 2 1\ncolor X ff0000  # red\ncolor A 0000ff  # new (painted)\nframe a\n..\nframe b\nAX\nanim x a:10 b:10\n");
        // Wrong size is refused.
        let small = Pixels { width: 1, height: 1, rgba: vec![0; 4], icc: None };
        assert!(apply_sheet(text, &small).unwrap_err().contains("frames can't be found"));
        // Painted from scratch on the blank layout: the palette is created before the frames.
        let empty = blank(text).unwrap();
        assert_eq!(empty, "# test\nsize 2 1\nframe a\n..\nframe b\n..\nanim x a:10 b:10\n");
        let fresh = apply_sheet(&empty, &img).unwrap();
        assert_eq!(fresh.text, "# test\nsize 2 1\ncolor A 0000ff  # new (painted)\ncolor B ff0000  # new (painted)\nframe a\n..\nframe b\nAB\nanim x a:10 b:10\n");
    }

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
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/pixel");
        for name in ["characters/wolfgang/desk.sprite", "characters/ully/desk.sprite", "characters/burgunde/desk.sprite", "src/ui.sprite", "src/wilma.sprite"] {
            let text = std::fs::read_to_string(root.join(name)).unwrap();
            let f = parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(apply_sheet(&text, &sheet(&f).0).unwrap().text, text, "{name}: PNG round trip changes the text");
        }
    }
}
