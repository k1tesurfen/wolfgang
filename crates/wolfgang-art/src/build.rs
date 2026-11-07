//! `wolfgang-art build`: everything in `assets/pixel/` from `assets/reference/`
//! (and, later, the code-drawn sprites).

use crate::pixelize::{self, Indexed};
use crate::sprite::{self, SheetFrame};
use serde::Serialize;
use std::collections::BTreeMap;
use crate::upscale;
use std::path::{Path, PathBuf};
use wolfgang_core::image::{decode, encode, ColorTarget, Pixels};
use wolfgang_core::ImageFormat;

/// Scene resolution: 16:9, one art pixel ≈ 8.6 reference pixels.
pub const SCENE_W: u32 = 320;
pub const SCENE_H: u32 = 180;
const OFFICE_COLORS: usize = 48;

/// Portraits: art pixel = 11.5 reference pixels; 96×96 head-and-shoulders crop.
const PORTRAIT_SIZE: u32 = 96;
const PORTRAIT_CELL: f64 = 11.5;
/// Top-left of the crop in reference pixels, aligned to the detected grid phase.
const PORTRAIT_ORIGIN: (f64, f64) = (894.8, 72.7);
const PORTRAIT_COLORS: usize = 40;
const BACKGROUND_TOLERANCE: f32 = 0.06;

/// (reference file, output name)
pub const PORTRAITS: &[(&str, &str)] = &[
    ("portrait-1-happy.jpeg", "happy"),
    ("portrait-2-thinking.jpeg", "thinking"),
    ("portrait-3-surprised.jpeg", "surprised"),
    ("portrait-4-content.jpeg", "content"),
    ("portrait-5-laughing.jpeg", "laughing"),
    ("portrait-6-sideeye.jpeg", "sideeye"),
    ("portrait-7-dreamy.jpeg", "dreamy"),
    ("portrait-8-sorry.jpeg", "sorry"),
];

pub struct Paths {
    pub reference: PathBuf,
    pub out: PathBuf,
    /// Optional folder for enlarged previews.
    pub preview: Option<PathBuf>,
}

fn write_png(path: &Path, p: &Pixels) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, encode(p, ImageFormat::Png, 0.0).expect("png")).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn to_pixels(i: &Indexed) -> Pixels {
    Pixels { width: i.width, height: i.height, rgba: i.rgba(), icc: None }
}

fn emit(paths: &Paths, name: &str, img: &Pixels, preview_scale: u32) {
    write_png(&paths.out.join(format!("{name}.png")), img);
    if let Some(dir) = &paths.preview {
        write_png(&dir.join(format!("{}.png", name.replace('/', "-"))), &upscale(img, preview_scale));
    }
    println!("  {name}.png  {}×{}", img.width, img.height);
}

pub fn office(paths: &Paths) -> Pixels {
    let img = decode(&paths.reference.join("office.jpeg"), None, ColorTarget::Srgb).expect("office reference");
    let full = (0, 0, img.width, img.height);
    let pal = pixelize::palette(&pixelize::colors(&img, full), OFFICE_COLORS);
    let px = to_pixels(&pixelize::pixelize(&img, full, SCENE_W, SCENE_H, &pal));
    emit(paths, "office", &px, 4);
    px
}

pub fn portraits(paths: &Paths) {
    let size = (PORTRAIT_SIZE as f64 * PORTRAIT_CELL).round() as u32;
    let region = (PORTRAIT_ORIGIN.0.round() as u32, PORTRAIT_ORIGIN.1.round() as u32, size, size);
    let refs: Vec<Pixels> = PORTRAITS
        .iter()
        .map(|(file, _)| decode(&paths.reference.join(file), None, ColorTarget::Srgb).unwrap_or_else(|e| panic!("{file}: {e}")))
        .collect();
    // One palette for all moods, so colours never jump between faces.
    let all: Vec<_> = refs.iter().flat_map(|r| pixelize::colors(r, region)).collect();
    let pal = pixelize::palette(&all, PORTRAIT_COLORS);
    let touchup_path = paths.out.join("src/portraits.touchup");
    let touchups = match std::fs::read_to_string(&touchup_path) {
        Ok(t) => parse_touchups(&t).unwrap_or_else(|e| panic!("{}: {e}", touchup_path.display())),
        Err(_) => BTreeMap::new(),
    };
    for name in touchups.keys() {
        assert!(PORTRAITS.iter().any(|(_, n)| n == name), "{}: unknown portrait '{name}'", touchup_path.display());
    }
    for (img, (_, name)) in refs.iter().zip(PORTRAITS) {
        let mut px = pixelize::pixelize(img, region, PORTRAIT_SIZE, PORTRAIT_SIZE, &pal);
        pixelize::cut_background(&mut px, BACKGROUND_TOLERANCE);
        let mut out = to_pixels(&px);
        for t in touchups.get(*name).into_iter().flatten() {
            for x in t.x0..=t.x1 {
                let o = ((t.y * out.width + x) * 4) as usize;
                out.rgba[o..o + 4].copy_from_slice(&t.rgba);
            }
        }
        emit(paths, &format!("portraits/{name}"), &out, 6);
    }
}

#[derive(Debug, PartialEq)]
pub struct Touchup {
    pub x0: u32,
    pub x1: u32,
    pub y: u32,
    pub rgba: [u8; 4],
}

/// Parses `portraits.touchup` (format described in the file itself).
pub fn parse_touchups(text: &str) -> Result<BTreeMap<String, Vec<Touchup>>, String> {
    let mut out: BTreeMap<String, Vec<Touchup>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let err = |m: &str| format!("line {}: {m} („{line}“)", n + 1);
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts[0] == "portrait" {
            current = Some(parts.get(1).ok_or_else(|| err("portrait needs a name"))?.to_string());
            continue;
        }
        let name = current.clone().ok_or_else(|| err("pixel before any `portrait` line"))?;
        let [xs, y, color] = parts[..] else { return Err(err("expected: X Y COLOR")) };
        let (x0, x1) = match xs.split_once('-') {
            Some((a, b)) => (a.parse().map_err(|_| err("bad x"))?, b.parse().map_err(|_| err("bad x"))?),
            None => {
                let x = xs.parse().map_err(|_| err("bad x"))?;
                (x, x)
            }
        };
        let y: u32 = y.parse().map_err(|_| err("bad y"))?;
        if x0 > x1 || x1 >= PORTRAIT_SIZE || y >= PORTRAIT_SIZE {
            return Err(err(&format!("outside the {PORTRAIT_SIZE}×{PORTRAIT_SIZE} portrait")));
        }
        let rgba = if color == "clear" {
            [0, 0, 0, 0]
        } else {
            let v = u32::from_str_radix(color, 16).ok().filter(|_| color.len() == 6).ok_or_else(|| err("colour must be RRGGBB or clear"))?;
            [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]
        };
        out.entry(name).or_default().push(Touchup { x0, x1, y, rgba });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touchups_parse() {
        let t = parse_touchups("portrait happy\n32 61 d5c1b2  # tooth\n33-40 61 eddcbf\nportrait sorry\n1 2 clear\n").unwrap();
        assert_eq!(t["happy"][1], Touchup { x0: 33, x1: 40, y: 61, rgba: [0xed, 0xdc, 0xbf, 255] });
        assert_eq!(t["sorry"][0].rgba, [0, 0, 0, 0]);
        assert!(parse_touchups("1 2 ffffff").unwrap_err().contains("before any"));
        assert!(parse_touchups("portrait x\n99 2 ffffff").unwrap_err().contains("outside"));
        assert!(parse_touchups("portrait x\n1 2 fff").unwrap_err().contains("RRGGBB"));
    }

    #[test]
    fn repo_touchups_are_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/pixel/src/portraits.touchup");
        let t = parse_touchups(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(t.keys().all(|k| PORTRAITS.iter().any(|(_, n)| n == k)));
    }
}

/// Renders a `.sprite` source into `<name>.png`; returns its manifest entry.
pub fn sprites(paths: &Paths, name: &str) -> SpriteManifest {
    let src = paths.out.join("src").join(format!("{name}.sprite"));
    let text = std::fs::read_to_string(&src).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
    let file = sprite::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", src.display()));
    let (sheet, frames) = sprite::sheet(&file);
    emit(paths, name, &sheet, 6);
    SpriteManifest { sheet: format!("{name}.png"), origin: file.origin.map(|(x, y)| [x, y]), frames, animations: file.anims }
}

#[derive(Serialize)]
pub struct SpriteManifest {
    pub sheet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<[i32; 2]>,
    pub frames: BTreeMap<String, SheetFrame>,
    pub animations: BTreeMap<String, Vec<(String, u32)>>,
}

#[derive(Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize)]
pub struct Hotspot {
    /// What clicking (or dropping on) it does in the app.
    pub action: &'static str,
    /// German tooltip.
    pub label: &'static str,
    pub rects: Vec<Rect>,
}

const fn r(x: u32, y: u32, w: u32, h: u32) -> Rect {
    Rect { x, y, w, h }
}

/// Everything the app needs to assemble the scene, in scene pixels (320×180).
#[derive(Serialize)]
pub struct Manifest {
    pub width: u32,
    pub height: u32,
    pub background: String,
    pub wolfgang: SpriteManifest,
    pub ui: SpriteManifest,
    pub portraits: BTreeMap<String, String>,
    /// Dialogue mood → portraits to pick from.
    pub moods: BTreeMap<String, Vec<String>>,
    pub hotspots: BTreeMap<String, Hotspot>,
    /// Where Wolfgang's speech bubbles may go, and the point their tail aims at (his mouth).
    pub bubble_area: Rect,
    pub speaker: [u32; 2],
}

pub fn manifest(paths: &Paths, wolfgang: SpriteManifest, ui: SpriteManifest) {
    let portraits = PORTRAITS.iter().map(|(_, n)| (n.to_string(), format!("portraits/{n}.png"))).collect();
    let moods = [
        ("neutral", vec!["sideeye", "content"]),
        ("happy", vec!["happy"]),
        ("thinking", vec!["thinking", "dreamy"]),
        ("surprised", vec!["surprised"]),
        ("proud", vec!["laughing", "content"]),
        ("sorry", vec!["sorry"]),
    ]
    .into_iter()
    .map(|(m, p)| (m.to_string(), p.into_iter().map(String::from).collect()))
    .collect();
    let hotspots = [
        ("desk", Hotspot { action: "drop", label: "Leg's auf den Tisch", rects: vec![r(73, 98, 180, 75)] }),
        ("cabinet", Hotspot { action: "settings", label: "Aktenschrank: Einstellungen", rects: vec![r(0, 112, 34, 68), r(84, 60, 26, 40)] }),
        ("clipboard", Hotspot { action: "presets", label: "Klemmbrett: Vorlagen", rects: vec![r(110, 106, 23, 15)] }),
        ("mug", Hotspot { action: "sip", label: "Wolfgangs Kaffee", rects: vec![r(100, 110, 9, 11)] }),
        ("computer", Hotspot { action: "about", label: "Computer: Über Wolfgang", rects: vec![r(207, 76, 38, 43)] }),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let m = Manifest {
        width: SCENE_W,
        height: SCENE_H,
        background: "office.png".into(),
        wolfgang,
        ui,
        portraits,
        moods,
        hotspots,
        bubble_area: r(56, 4, 208, 46),
        speaker: [162, 70],
    };
    let path = paths.out.join("scene.json");
    let json = serde_json::to_string_pretty(&m).unwrap();
    std::fs::write(&path, json.clone() + "\n").unwrap();
    println!("  scene.json");
    preview(paths, &json, &m);
}

/// A self-contained HTML page (images inlined) to look at the scene in a browser.
fn preview(paths: &Paths, scene_json: &str, m: &Manifest) {
    let mut files: Vec<String> = vec![m.background.clone(), m.wolfgang.sheet.clone(), m.ui.sheet.clone()];
    files.extend(m.portraits.values().cloned());
    let images: BTreeMap<String, String> = files
        .into_iter()
        .map(|f| {
            let bytes = std::fs::read(paths.out.join(&f)).unwrap();
            (f, format!("data:image/png;base64,{}", base64(&bytes)))
        })
        .collect();
    let html = include_str!("preview.html")
        .replace("/*SCENE*/{}", scene_json)
        .replace("/*IMAGES*/{}", &serde_json::to_string(&images).unwrap());
    let path = PathBuf::from("target/wolfgang-art/preview.html");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, html).unwrap();
    println!("\npreview: {}", std::fs::canonicalize(&path).unwrap().display());
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}
