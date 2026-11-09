//! The art pipeline.
//!
//! - `import`: turns the reference images into pixel art, **once**. The results
//!   (`src/office.png`, `src/portraits/*.png`) are then hand-made sources like the
//!   `.sprite` files; `import` refuses to overwrite them without `--force`.
//! - `build`: hand-made sources in `assets/pixel/src/` → sprite sheets,
//!   `scene.json` and the preview. It never touches the sources.

use crate::pixelize::{self, Indexed};
use crate::sprite::{self, SheetFrame};
use crate::upscale;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use wolfgang_core::image::{decode, encode, ColorTarget, Pixels};
use wolfgang_core::ImageFormat;

/// Scene resolution: 16:9, one art pixel ≈ 8.6 reference pixels.
pub const SCENE_W: u32 = 320;
pub const SCENE_H: u32 = 180;
const OFFICE_COLORS: usize = 48;

/// Portraits: art pixel = 11.5 reference pixels; 96×96 head-and-shoulders crop.
pub const PORTRAIT_SIZE: u32 = 96;
const PORTRAIT_CELL: f64 = 11.5;
/// Top-left of the crop in reference pixels, aligned to the detected grid phase.
const PORTRAIT_ORIGIN: (f64, f64) = (894.8, 72.7);
const PORTRAIT_COLORS: usize = 40;
const BACKGROUND_TOLERANCE: f32 = 0.06;

/// More distinct colours than this in a hand-made source usually means an editor
/// smoothed something (anti-aliasing, interpolated scaling).
const MAX_COLORS: usize = 128;

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

impl Paths {
    fn src(&self) -> PathBuf {
        self.out.join("src")
    }
    fn office(&self) -> PathBuf {
        self.src().join("office.png")
    }
    fn portrait(&self, name: &str) -> PathBuf {
        self.src().join("portraits").join(format!("{name}.png"))
    }
}

fn write_png(path: &Path, p: &Pixels) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, encode(p, ImageFormat::Png, 0.0).expect("png")).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn to_pixels(i: &Indexed) -> Pixels {
    Pixels { width: i.width, height: i.height, rgba: i.rgba(), icc: None }
}

/// Writes a generated file into `assets/pixel/` (plus an enlarged preview copy).
fn emit(paths: &Paths, name: &str, img: &Pixels, preview_scale: u32) {
    write_png(&paths.out.join(format!("{name}.png")), img);
    if let Some(dir) = &paths.preview {
        write_png(&dir.join(format!("{}.png", name.replace('/', "-"))), &upscale(img, preview_scale));
    }
    println!("  {name}.png  {}×{}", img.width, img.height);
}

/// Converts the references into `src/office.png` and `src/portraits/*.png`.
pub fn import(paths: &Paths, force: bool) -> Result<(), String> {
    let targets: Vec<PathBuf> = std::iter::once(paths.office()).chain(PORTRAITS.iter().map(|(_, n)| paths.portrait(n))).collect();
    let existing: Vec<String> = targets.iter().filter(|t| t.exists()).map(|t| t.display().to_string()).collect();
    if !existing.is_empty() && !force {
        return Err(format!(
            "These already exist and may contain hand-drawn pixels:\n  {}\nUse --force to overwrite them with fresh conversions.",
            existing.join("\n  ")
        ));
    }

    let img = decode(&paths.reference.join("office.jpeg"), None, ColorTarget::Srgb).map_err(|e| e.to_string())?;
    let full = (0, 0, img.width, img.height);
    let pal = pixelize::palette(&pixelize::colors(&img, full), OFFICE_COLORS);
    write_png(&paths.office(), &to_pixels(&pixelize::pixelize(&img, full, SCENE_W, SCENE_H, &pal)));
    println!("  {}", paths.office().display());

    let size = (PORTRAIT_SIZE as f64 * PORTRAIT_CELL).round() as u32;
    let region = (PORTRAIT_ORIGIN.0.round() as u32, PORTRAIT_ORIGIN.1.round() as u32, size, size);
    let refs = PORTRAITS
        .iter()
        .map(|(file, _)| decode(&paths.reference.join(file), None, ColorTarget::Srgb).map_err(|e| format!("{file}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    // One palette for all moods, so colours never jump between faces.
    let all: Vec<_> = refs.iter().flat_map(|r| pixelize::colors(r, region)).collect();
    let pal = pixelize::palette(&all, PORTRAIT_COLORS);
    for (img, (_, name)) in refs.iter().zip(PORTRAITS) {
        let mut px = pixelize::pixelize(img, region, PORTRAIT_SIZE, PORTRAIT_SIZE, &pal);
        pixelize::cut_background(&mut px, BACKGROUND_TOLERANCE);
        write_png(&paths.portrait(name), &to_pixels(&px));
        println!("  {}", paths.portrait(name).display());
    }
    Ok(())
}

/// Reads a hand-made PNG exactly as stored (no colour management).
fn read_png(path: &Path) -> Result<Pixels, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(format!("{}: unexpected indexed colour after expansion", path.display())),
    };
    Ok(Pixels { width: info.width, height: info.height, rgba, icc: None })
}

/// Problems that would look wrong in the scene: the wrong size is an error;
/// half-transparent pixels and very many colours are warnings (smoothing editors).
pub fn check_source(img: &Pixels, w: u32, h: u32) -> Result<Vec<String>, String> {
    if (img.width, img.height) != (w, h) {
        return Err(format!("is {}×{}, expected {w}×{h}", img.width, img.height));
    }
    let mut warnings = Vec::new();
    let soft = img.rgba.chunks_exact(4).filter(|p| p[3] != 0 && p[3] != 255).count();
    if soft > 0 {
        warnings.push(format!("{soft} half-transparent pixels (pixel art should be fully opaque or fully transparent)"));
    }
    let colors: HashSet<[u8; 3]> = img.rgba.chunks_exact(4).filter(|p| p[3] != 0).map(|p| [p[0], p[1], p[2]]).collect();
    if colors.len() > MAX_COLORS {
        warnings.push(format!("{} colours: was it smoothed or scaled with interpolation?", colors.len()));
    }
    Ok(warnings)
}

/// Checks the hand-made PNG sources; panics on errors, prints warnings.
pub fn check_sources(paths: &Paths) {
    let mut list = vec![(paths.office(), SCENE_W, SCENE_H)];
    list.extend(PORTRAITS.iter().map(|(_, n)| (paths.portrait(n), PORTRAIT_SIZE, PORTRAIT_SIZE)));
    for (path, w, h) in list {
        let img = read_png(&path).unwrap_or_else(|e| panic!("{e}\n(Run `wolfgang-art import` to create missing sources.)"));
        match check_source(&img, w, h) {
            Ok(warnings) => {
                for warning in warnings {
                    println!("  ⚠ {}: {warning}", path.display());
                }
            }
            Err(e) => panic!("{}: {e}", path.display()),
        }
    }
    println!("  sources ok: office + {} portraits", PORTRAITS.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32, f: impl Fn(u32) -> [u8; 4]) -> Pixels {
        Pixels { width: w, height: h, rgba: (0..w * h).flat_map(f).collect(), icc: None }
    }

    #[test]
    fn source_checks() {
        assert!(check_source(&img(4, 4, |_| [1, 2, 3, 255]), 4, 4).unwrap().is_empty());
        assert!(check_source(&img(4, 4, |_| [1, 2, 3, 255]), 5, 4).unwrap_err().contains("expected 5×4"));
        let soft = check_source(&img(4, 4, |i| [1, 2, 3, if i == 0 { 128 } else { 255 }]), 4, 4).unwrap();
        assert!(soft[0].contains("1 half-transparent"));
        let many = check_source(&img(20, 20, |i| [i as u8, 0, 0, 255]), 20, 20).unwrap();
        assert!(many[0].contains("smoothed"));
    }

    #[test]
    fn repo_sources_pass_the_checks() {
        let paths = Paths { reference: PathBuf::new(), out: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/pixel"), preview: None };
        let office = read_png(&paths.office()).unwrap();
        assert!(check_source(&office, SCENE_W, SCENE_H).unwrap().is_empty());
        for (_, n) in PORTRAITS {
            let p = read_png(&paths.portrait(n)).unwrap();
            assert!(check_source(&p, PORTRAIT_SIZE, PORTRAIT_SIZE).unwrap().is_empty(), "{n}");
        }
    }

    #[test]
    fn import_never_overwrites_without_force() {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths { reference: t.path().join("ref"), out: t.path().to_path_buf(), preview: None };
        std::fs::create_dir_all(paths.src()).unwrap();
        std::fs::write(paths.office(), "hand-drawn").unwrap();
        let err = import(&paths, false).unwrap_err();
        assert!(err.contains("office.png") && err.contains("--force"), "{err}");
        assert_eq!(std::fs::read_to_string(paths.office()).unwrap(), "hand-drawn");
    }
}

/// The app icon: the happy portrait in a rounded, framed square (macOS style),
/// built at 128 px and scaled 8× without smoothing to 1024 px.
pub fn icon(paths: &Paths, out: &Path) {
    const N: i32 = 128;
    const R: i32 = 22; // corner radius
    let face = read_png(&paths.portrait("happy")).expect("happy portrait");
    let (x0, y0, x1, y1) = (8, 8, N - 9, N - 9); // the rounded square
    let inside = |x: i32, y: i32, inset: i32| {
        let (ax0, ay0, ax1, ay1, r) = (x0 + inset, y0 + inset, x1 - inset, y1 - inset, R - inset);
        if x < ax0 || x > ax1 || y < ay0 || y > ay1 {
            return false;
        }
        let cx = x.clamp(ax0 + r, ax1 - r);
        let cy = y.clamp(ay0 + r, ay1 - r);
        (x - cx).pow(2) + (y - cy).pow(2) <= r * r
    };
    let hex = |v: u32| [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255];
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    // Portrait bottom-aligned on the frame's inner edge, centred.
    let (fx, fy) = ((N - 96) / 2, y1 - 3 - 95);
    for y in 0..N {
        for x in 0..N {
            let c = if !inside(x, y, 0) {
                [0, 0, 0, 0]
            } else if !inside(x, y, 1) {
                hex(0x1c0c16) // outline
            } else if !inside(x, y, 2) {
                hex(0xa8703f) // wood light
            } else if !inside(x, y, 3) {
                hex(0x5c3a22) // wood dark
            } else {
                let (px, py) = (x - fx, y - fy);
                let p = if (0..96).contains(&px) && (0..96).contains(&py) { ((py * 96 + px) * 4) as usize } else { usize::MAX };
                if p != usize::MAX && face.rgba[p + 3] == 255 { [face.rgba[p], face.rgba[p + 1], face.rgba[p + 2], 255] } else { hex(0xdccab0) }
            };
            rgba[((y * N + x) * 4) as usize..][..4].copy_from_slice(&c);
        }
    }
    let small = Pixels { width: N as u32, height: N as u32, rgba, icc: None };
    write_png(out, &upscale(&small, 8));
    println!("  {}  1024×1024", out.display());
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
    /// Wilma, the office cat: `origin` is her cushion (bottom centre).
    pub wilma: SpriteManifest,
    /// The strip of floor in front of the desk where she wanders: x from–to, and the y of her paws.
    pub wilma_floor: [u32; 3],
    pub portraits: BTreeMap<String, String>,
    /// Dialogue mood → portraits to pick from.
    pub moods: BTreeMap<String, Vec<String>>,
    pub hotspots: BTreeMap<String, Hotspot>,
    /// Where Wolfgang's speech bubbles may go, and the point their tail aims at (his mouth).
    pub bubble_area: Rect,
    pub speaker: [u32; 2],
}

pub fn manifest(paths: &Paths, wolfgang: SpriteManifest, ui: SpriteManifest, wilma: SpriteManifest) {
    let portraits = PORTRAITS.iter().map(|(_, n)| (n.to_string(), format!("src/portraits/{n}.png"))).collect();
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
        // Everything interactive sits on or at the desk.
        // The whole desk: click → Finder dialog, and it's where drops light up. The first
        // rect is the back edge of the green desk mat (a trapezoid), where the sparkle shows
        // while hovering the desk.
        ("desk", Hotspot { action: "pick", label: "Dateien auswählen … (⌘O)", rects: vec![r(132, 101, 65, 19), r(73, 98, 180, 75)] }),
        ("wolfgang", Hotspot { action: "about", label: "Wolfgang: Über Wolfgang", rects: vec![r(145, 52, 36, 46)] }),
        ("computer", Hotspot { action: "settings", label: "Computer: Einstellungen", rects: vec![r(207, 76, 38, 43)] }),
        ("clipboard", Hotspot { action: "presets", label: "Klemmbrett: Vorlagen", rects: vec![r(110, 106, 23, 15)] }),
        ("mug", Hotspot { action: "sip", label: "Wolfgangs Kaffee", rects: vec![r(100, 110, 9, 11)] }),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let m = Manifest {
        width: SCENE_W,
        height: SCENE_H,
        background: "src/office.png".into(),
        wolfgang,
        ui,
        wilma,
        wilma_floor: [116, 232, 178],
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
    let mut files: Vec<String> = vec![m.background.clone(), m.wolfgang.sheet.clone(), m.ui.sheet.clone(), m.wilma.sheet.clone()];
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
