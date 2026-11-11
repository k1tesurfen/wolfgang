//! The art pipeline.
//!
//! - `import` / `import-character`: turn the reference images into pixel art, **once**.
//!   The results are then hand-made sources; they are never overwritten without `--force`.
//! - `build`: hand-made sources → sprite sheets, `scene.json` and the preview. Shared art
//!   lives in `assets/pixel/src/`, each character's in `assets/pixel/characters/<key>/`
//!   (the exchange folder: portraits, desk sprite, optional office). The only source it
//!   writes is a character's `desk.png` ↔ `desk.sprite` pair, which it keeps identical.

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

/// The moods every character has a portrait for, in this order.
pub const MOODS: [&str; 8] = ["happy", "thinking", "surprised", "content", "laughing", "sideeye", "dreamy", "sorry"];
/// A missing portrait shows one of these (the first the character has) instead.
const STAND_INS: &[(&str, &[&str])] = &[
    ("happy", &["content", "laughing"]),
    ("thinking", &["dreamy", "content"]),
    ("surprised", &["content", "happy"]),
    ("content", &["happy", "sideeye"]),
    ("laughing", &["happy", "content"]),
    ("sideeye", &["content", "thinking"]),
    ("dreamy", &["thinking", "content"]),
    ("sorry", &["content", "thinking"]),
];
/// Colours of a newly imported portrait this close (OKLab) to one the character
/// already has take the existing one, so the palette only grows by really new colours.
const SAME_COLOR: f32 = 0.035;

/// Wolfgang's references: (reference file, mood)
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
    /// A character's exchange folder.
    fn character(&self, key: &str) -> PathBuf {
        self.out.join("characters").join(key)
    }
    fn portrait(&self, key: &str, mood: &str) -> PathBuf {
        self.character(key).join("portraits").join(format!("{mood}.png"))
    }
}

/// `path` relative to `assets/pixel/`, as `scene.json` and the app use it.
fn rel(paths: &Paths, path: &Path) -> String {
    path.strip_prefix(&paths.out).unwrap_or(path).to_string_lossy().into_owned()
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

/// Converts Wolfgang's references into `src/office.png` and his portraits.
pub fn import(paths: &Paths, force: bool) -> Result<(), String> {
    let targets: Vec<PathBuf> = std::iter::once(paths.office()).chain(PORTRAITS.iter().map(|(_, n)| paths.portrait("wolfgang", n))).collect();
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
        write_png(&paths.portrait("wolfgang", name), &to_pixels(&px));
        println!("  {}", paths.portrait("wolfgang", name).display());
    }
    Ok(())
}

/// A portrait reference read cell by cell: Gemini's grid is found, every cell takes the
/// colour of its centre. Returns the cells (`n`×`n`) and how many make one portrait pixel.
fn portrait_cells(img: &Pixels) -> (Vec<pixelize::Rgb>, u32, u32) {
    let g = crate::grid::find(img, (0, 0, img.width, img.height), 3.0, 30.0);
    let f = (PORTRAIT_CELL / g.x.period).round().max(1.0) as u32;
    let cells = pixelize::Cells { period: (g.x.period, g.y.period), phase: (g.x.phase, g.y.phase) };
    let first = (((PORTRAIT_ORIGIN.0 - g.x.phase) / g.x.period).round() as i64, ((PORTRAIT_ORIGIN.1 - g.y.phase) / g.y.period).round() as i64);
    let n = PORTRAIT_SIZE * f;
    (pixelize::cell_colors(img, &cells, first, (n, n)), n, f)
}

/// Cells → a 96×96 portrait in `pal`, background cut away.
fn portrait_from_cells(cells: &[pixelize::Rgb], n: u32, f: u32, pal: &[pixelize::Rgb]) -> Indexed {
    let lab: Vec<[f32; 3]> = pal.iter().map(|c| pixelize::oklab(*c)).collect();
    let index: Vec<u16> = cells.iter().map(|c| pixelize::nearest(&lab, &pixelize::oklab(*c))).collect();
    let mut px = Indexed { width: PORTRAIT_SIZE, height: PORTRAIT_SIZE, index: pixelize::reduce(&index, n, n, f, &lab, index.first().copied()), palette: pal.to_vec() };
    pixelize::cut_background(&mut px, BACKGROUND_TOLERANCE);
    px
}

/// The reference for a mood in `assets/reference/<key>/`, if there is one.
fn mood_reference(paths: &Paths, key: &str, mood: &str) -> Option<PathBuf> {
    ["png", "jpeg", "jpg"].iter().map(|ext| paths.reference.join(key).join(format!("{mood}.{ext}"))).find(|p| p.exists())
}

/// `assets/reference/<key>/<mood>.jpeg` → `characters/<key>/portraits/<mood>.png`, cell by
/// cell. Portraits that exist are kept (`force` redoes them). The palette grows: a colour
/// close to one the character's existing portraits use becomes that colour.
pub fn import_character(paths: &Paths, key: &str, force: bool) -> Result<(), String> {
    let mut todo = Vec::new();
    for mood in MOODS {
        match (mood_reference(paths, key, mood), paths.portrait(key, mood).exists() && !force) {
            (Some(_), true) => println!("  {mood}: portrait exists, kept (--force converts it again)"),
            (Some(r), false) => todo.push((mood, r)),
            (None, _) => {}
        }
    }
    if todo.is_empty() {
        return Err(format!("nothing to convert: no new <mood>.jpeg in {} ({})", paths.reference.join(key).display(), MOODS.join(", ")));
    }
    let pal_path = paths.character(key).join("palette.hex");
    let existing: Vec<pixelize::Rgb> = if force {
        Vec::new()
    } else if let Some(pal) = read_palette(&pal_path) {
        pal
    } else {
        let mut set = std::collections::BTreeSet::new();
        for mood in MOODS {
            if let Ok(p) = read_png(&paths.portrait(key, mood)) {
                set.extend(p.rgba.chunks_exact(4).filter(|p| p[3] == 255).map(|p| [p[0], p[1], p[2]]));
            }
        }
        set.into_iter().collect()
    };
    let refs = todo
        .iter()
        .map(|(_, r)| decode(r, None, ColorTarget::Srgb).map(|img| portrait_cells(&img)).map_err(|e| format!("{}: {e}", r.display())))
        .collect::<Result<Vec<_>, _>>()?;
    // One palette for all new moods, snapped onto the colours the character already has.
    let all: Vec<_> = refs.iter().flat_map(|(cells, _, _)| cells.iter().copied()).collect();
    let fresh = pixelize::palette(&all, PORTRAIT_COLORS);
    let pal = grow_palette(&existing, &fresh);
    println!("  palette: {} colours ({} new)", pal.len(), pal.len() - existing.len());
    for ((mood, r), (cells, n, f)) in todo.iter().zip(&refs) {
        let px = portrait_from_cells(cells, *n, *f, &pal);
        write_png(&paths.portrait(key, mood), &to_pixels(&px));
        println!("  {mood}: {} → {}", r.file_name().unwrap().to_string_lossy(), rel(paths, &paths.portrait(key, mood)));
    }
    write_palette(&pal_path, &pal);
    Ok(())
}

/// A colour this close (largest difference of R, G, B) to a palette colour is that colour:
/// editors and websites (Pixilart) shift colours by a few steps when saving. Burgunde's
/// closest real pair is 7 apart, the saving noise up to ~10; snapping goes to the
/// *nearest* palette colour, so close real pairs stay apart.
const SNAP: i32 = 12;

fn hex(c: pixelize::Rgb) -> String {
    format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A `.hex` palette (one RRGGBB per line, as Aseprite, Pixelorama and Lospec read it).
fn read_palette(path: &Path) -> Option<Vec<pixelize::Rgb>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter_map(|l| u32::from_str_radix(l.trim().trim_start_matches('#'), 16).ok())
            .map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
            .collect(),
    )
}

fn write_palette(path: &Path, pal: &[pixelize::Rgb]) {
    std::fs::write(path, pal.iter().map(|c| hex(*c) + "\n").collect::<String>()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

/// Snaps every pixel onto `pal`: the nearest colour if it is within [`SNAP`], else the
/// colour is new and joins `pal` (most used first, so a new colour's own noisy variants
/// snap onto it). Half-transparent pixels become opaque or transparent.
/// Returns the colours added.
fn snap(img: &mut Pixels, pal: &mut Vec<pixelize::Rgb>) -> Vec<pixelize::Rgb> {
    let d = |a: &pixelize::Rgb, b: &pixelize::Rgb| (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).max().unwrap();
    let mut count: BTreeMap<pixelize::Rgb, usize> = BTreeMap::new();
    for p in img.rgba.chunks_exact(4).filter(|p| p[3] >= 128) {
        *count.entry([p[0], p[1], p[2]]).or_default() += 1;
    }
    let mut by_use: Vec<(pixelize::Rgb, usize)> = count.into_iter().collect();
    by_use.sort_by_key(|(c, n)| (std::cmp::Reverse(*n), *c));
    let mut to: BTreeMap<pixelize::Rgb, pixelize::Rgb> = BTreeMap::new();
    let mut added = Vec::new();
    for (c, _) in by_use {
        let target = match pal.iter().min_by_key(|p| d(p, &c)) {
            Some(p) if d(p, &c) <= SNAP => *p,
            _ => {
                pal.push(c);
                added.push(c);
                c
            }
        };
        to.insert(c, target);
    }
    for p in img.rgba.chunks_exact_mut(4) {
        if p[3] < 128 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        } else {
            let t = to[&[p[0], p[1], p[2]]];
            p.copy_from_slice(&[t[0], t[1], t[2], 255]);
        }
    }
    added
}

/// Snaps every character's portraits onto their `palette.hex` (rewriting the PNGs that
/// change) and adds really new colours to it. Without a `palette.hex` it is started from
/// the portraits: all their colours if they are clean, else grown by snapping.
pub fn clean_portraits(paths: &Paths) {
    for c in wolfgang_core::character::all() {
        let files: Vec<PathBuf> = MOODS.iter().map(|m| paths.portrait(&c.key, m)).filter(|p| p.exists()).collect();
        if files.is_empty() {
            continue;
        }
        let pal_path = paths.character(&c.key).join("palette.hex");
        let mut pal = read_palette(&pal_path).unwrap_or_else(|| {
            let mut all = std::collections::BTreeSet::new();
            for f in &files {
                let img = read_png(f).unwrap_or_else(|e| panic!("{e}"));
                all.extend(img.rgba.chunks_exact(4).filter(|p| p[3] >= 128).map(|p| [p[0], p[1], p[2]]));
            }
            // Clean art keeps every colour it has; noisy art starts empty and grows below.
            if all.len() <= MAX_COLORS { all.into_iter().collect() } else { Vec::new() }
        });
        let before = pal.clone();
        for f in &files {
            let mut img = read_png(f).unwrap_or_else(|e| panic!("{e}"));
            let original = img.rgba.clone();
            let colors = |rgba: &[u8]| rgba.chunks_exact(4).filter(|p| p[3] != 0).map(|p| [p[0], p[1], p[2]]).collect::<HashSet<_>>().len();
            let added = snap(&mut img, &mut pal);
            if img.rgba != original {
                let moved = original.chunks_exact(4).zip(img.rgba.chunks_exact(4)).filter(|(a, b)| a != b).count();
                write_png(f, &img);
                println!(
                    "  {}: {} colours → {} ({moved} pixels snapped onto the palette{})",
                    rel(paths, f),
                    colors(&original),
                    colors(&img.rgba),
                    if added.is_empty() { String::new() } else { format!(", new: {}", added.iter().map(|c| hex(*c)).collect::<Vec<_>>().join(" ")) }
                );
            }
        }
        if pal != before || !pal_path.exists() {
            write_palette(&pal_path, &pal);
            println!("  {}: {} colours", rel(paths, &pal_path), pal.len());
        }
    }
}

/// `existing` plus every colour of `fresh` that isn't close to one of them.
fn grow_palette(existing: &[pixelize::Rgb], fresh: &[pixelize::Rgb]) -> Vec<pixelize::Rgb> {
    let lab: Vec<[f32; 3]> = existing.iter().map(|c| pixelize::oklab(*c)).collect();
    let mut pal = existing.to_vec();
    for c in fresh {
        let l = pixelize::oklab(*c);
        let near = lab.iter().any(|e| (0..3).map(|i| (e[i] - l[i]).powi(2)).sum::<f32>() < SAME_COLOR * SAME_COLOR);
        if !near && !pal.contains(c) {
            pal.push(*c);
        }
    }
    pal
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

/// Checks the hand-made PNG sources (shared office, every character's portraits and
/// office); panics on errors, prints warnings.
pub fn check_sources(paths: &Paths) {
    let mut list = vec![(paths.office(), SCENE_W, SCENE_H)];
    for c in wolfgang_core::character::all() {
        list.extend(MOODS.iter().map(|m| (paths.portrait(&c.key, m), PORTRAIT_SIZE, PORTRAIT_SIZE)).filter(|(p, _, _)| p.exists()));
        list.extend(Some((paths.character(&c.key).join("office.png"), SCENE_W, SCENE_H)).filter(|(p, _, _)| p.exists()));
    }
    // Wolfgang is the fallback for everyone: his set must be complete.
    for m in MOODS {
        if !paths.portrait("wolfgang", m).exists() {
            panic!("{} is missing (Wolfgang's portraits stand in for missing ones of the others)", paths.portrait("wolfgang", m).display());
        }
    }
    let n = list.len();
    for (path, w, h) in list {
        let img = read_png(&path).unwrap_or_else(|e| panic!("{e}"));
        match check_source(&img, w, h) {
            Ok(warnings) => {
                for warning in warnings {
                    println!("  ⚠ {}: {warning}", rel(paths, &path));
                }
            }
            Err(e) => panic!("{}: {e}", path.display()),
        }
    }
    println!("  sources ok: {n} PNGs");
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
        for c in wolfgang_core::character::all() {
            for m in MOODS {
                if let Ok(p) = read_png(&paths.portrait(&c.key, m)) {
                    assert!(check_source(&p, PORTRAIT_SIZE, PORTRAIT_SIZE).unwrap().is_empty(), "{} {m}", c.key);
                }
            }
        }
    }

    #[test]
    fn missing_portraits_get_stand_ins() {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths { reference: PathBuf::new(), out: t.path().to_path_buf(), preview: None };
        let face = img(1, 1, |_| [1, 2, 3, 255]);
        for m in MOODS {
            write_png(&paths.portrait("wolfgang", m), &face);
        }
        write_png(&paths.portrait("ully", "content"), &face);
        write_png(&paths.portrait("ully", "thinking"), &face);
        let p = character_portraits(&paths, "ully");
        assert_eq!(p["content"], "characters/ully/portraits/content.png");
        assert_eq!(p["happy"], "characters/ully/portraits/content.png");
        assert_eq!(p["dreamy"], "characters/ully/portraits/thinking.png");
        assert_eq!(p["sorry"], "characters/ully/portraits/content.png");
        // Nothing of their own: Wolfgang's.
        assert_eq!(character_portraits(&paths, "burgunde")["sorry"], "characters/wolfgang/portraits/sorry.png");
    }

    #[test]
    fn saving_noise_snaps_onto_the_palette() {
        // Two real colours only 7 apart, each saved a few steps off; one clearly new colour.
        let mut pal = vec![[100, 100, 100], [107, 100, 100]];
        let mut p = img(5, 1, |i| [[101, 99, 100, 255], [108, 101, 99, 250], [200, 10, 10, 255], [203, 12, 9, 255], [9, 9, 9, 3]][i as usize]);
        let added = snap(&mut p, &mut pal);
        assert_eq!(added, vec![[200, 10, 10]]);
        assert_eq!(p.rgba, [[100, 100, 100, 255], [107, 100, 100, 255], [200, 10, 10, 255], [200, 10, 10, 255], [0, 0, 0, 0]].concat());
        assert_eq!(pal.len(), 3);
    }

    fn sprite_text() -> &'static str {
        "size 2 1\ncolor X ff0000\nframe a\nX.\nanim x a:0\n"
    }

    #[test]
    fn desk_png_and_text_stay_in_sync() {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths { reference: PathBuf::new(), out: t.path().to_path_buf(), preview: None };
        let dir = paths.character("ully");
        std::fs::create_dir_all(&dir).unwrap();
        let (png, text) = (dir.join("desk.png"), dir.join("desk.sprite"));
        // Text only: the PNG is written.
        std::fs::write(&text, sprite_text()).unwrap();
        assert!(sync_desk(&paths, "ully").unwrap().is_some());
        // One frame of 2 px plus the 1 px gap after it.
        assert_eq!(read_png(&png).unwrap().rgba, vec![255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        // Painted: the text follows, with the new colour.
        write_png(&png, &img(3, 1, |i| [[255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 0, 0]][i as usize]));
        sync_desk(&paths, "ully").unwrap();
        assert!(std::fs::read_to_string(&text).unwrap().contains("XA"));
        // Text edited: the PNG follows.
        std::fs::write(&text, std::fs::read_to_string(&text).unwrap().replace("XA", "AA")).unwrap();
        sync_desk(&paths, "ully").unwrap();
        assert_eq!(&read_png(&png).unwrap().rgba[..4], &[0, 0, 255, 255]);
        // Both changed: refused, nothing touched.
        std::fs::write(&text, std::fs::read_to_string(&text).unwrap().replace("AA", "A.")).unwrap();
        write_png(&png, &img(3, 1, |i| if i < 2 { [255, 0, 0, 255] } else { [0, 0, 0, 0] }));
        let err = sync_desk(&paths, "ully").unwrap_err();
        assert!(err.contains("both changed"), "{err}");
        assert!(std::fs::read_to_string(&text).unwrap().contains("A."));
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
pub fn icon(paths: &Paths, key: &str, out: &Path) {
    // Apple's icon template: the shape is 824 of 1024 px, with continuous („squircle“)
    // corners and a soft shadow below. One art pixel is 10 px, so Wolfgang's face is
    // bigger than in the scene; the portrait is cropped around head and beard.
    const UNIT: i32 = 10;
    const N: i32 = 1024 / UNIT;
    const S: f64 = 824.0 / UNIT as f64;
    const CROP: (i32, i32) = (10, 3); // top-left of the portrait window in the 96×96 portrait
    // The wooden frame from the outside in: outline, light, middle, dark wood.
    const FRAME: [u32; 4] = [0x1c0c16, 0xb8804a, 0x8a5a33, 0x5c3a22];
    let face = read_png(&paths.portrait(key, "happy")).expect("happy portrait");
    let s = S.round();
    let o = (N as f64 - s) / 2.0;
    let inside = |x: i32, y: i32, inset: f64, dy: f64| {
        let h = s / 2.0 - inset;
        let (cx, cy) = (o + s / 2.0, o + s / 2.0 + dy);
        let (nx, ny) = (((x as f64 + 0.5) - cx).abs() / h, ((y as f64 + 0.5) - cy).abs() / h);
        nx.powi(5) + ny.powi(5) <= 1.0
    };
    let hex = |v: u32| [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255];
    let window = o.round() as i32 + FRAME.len() as i32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let c = if inside(x, y, 0.0, 0.0) {
                match (0..FRAME.len()).find(|&i| !inside(x, y, i as f64 + 1.0, 0.0)) {
                    Some(i) => hex(FRAME[i]),
                    None => {
                        let (px, py) = (x - window + CROP.0, y - window + CROP.1);
                        let p = ((py * 96 + px) * 4) as usize;
                        if (0..96).contains(&px) && (0..96).contains(&py) && face.rgba[p + 3] == 255 {
                            [face.rgba[p], face.rgba[p + 1], face.rgba[p + 2], 255]
                        } else {
                            hex(0xdccab0)
                        }
                    }
                }
            } else if inside(x, y, 0.0, 1.5) {
                [0, 0, 0, 70] // shadow
            } else {
                [0, 0, 0, 0]
            };
            rgba[((y * N + x) * 4) as usize..][..4].copy_from_slice(&c);
        }
    }
    let art = upscale(&Pixels { width: N as u32, height: N as u32, rgba, icc: None }, UNIT as u32);
    // 102 × 10 = 1020: centre it on the 1024 canvas.
    let mut full = vec![0u8; 1024 * 1024 * 4];
    let off = ((1024 - art.width) / 2) as usize;
    for y in 0..art.height as usize {
        let src = &art.rgba[y * art.width as usize * 4..(y + 1) * art.width as usize * 4];
        let dst = ((y + off) * 1024 + off) * 4;
        full[dst..dst + src.len()].copy_from_slice(src);
    }
    write_png(out, &Pixels { width: 1024, height: 1024, rgba: full, icc: None });
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

/// A whole reference image at another pixel size (`pixel` reference pixels per art
/// pixel, e.g. 34.5 for the desk sprite's scale): cell by cell, then reduced like the
/// portraits. A drawing aid for sprites, written to `out` at 1×.
pub fn downscale(path: &Path, pixel: f64, colors: usize, out: &Path) -> Result<(), String> {
    let img = decode(path, None, ColorTarget::Srgb).map_err(|e| format!("{}: {e}", path.display()))?;
    let g = crate::grid::find(&img, (0, 0, img.width, img.height), 3.0, 30.0);
    let f = (pixel / g.x.period).round().max(1.0) as u32;
    let cells = pixelize::Cells { period: (g.x.period, g.y.period), phase: (g.x.phase, g.y.phase) };
    let (w, h) = ((img.width as f64 / (g.x.period * f as f64)) as u32, (img.height as f64 / (g.y.period * f as f64)) as u32);
    let (nw, nh) = (w * f, h * f);
    let native = pixelize::cell_colors(&img, &cells, (0, 0), (nw, nh));
    let pal = pixelize::palette(&native, colors);
    let lab: Vec<[f32; 3]> = pal.iter().map(|c| pixelize::oklab(*c)).collect();
    let index: Vec<u16> = native.iter().map(|c| pixelize::nearest(&lab, &pixelize::oklab(*c))).collect();
    let mut px = Indexed { width: w, height: h, index: pixelize::reduce(&index, nw, nh, f, &lab, index.first().copied()), palette: pal };
    pixelize::cut_background(&mut px, BACKGROUND_TOLERANCE);
    write_png(out, &to_pixels(&px));
    println!("  {}: grid {:.2} px × {f} → {w}×{h}", out.display(), g.x.period);
    Ok(())
}

/// Experiment: one reference image → 96×96 portrait, current method next to the
/// cell-based one (Gemini's grid sampled cell by cell, then reduced if it is finer
/// than a portrait pixel). Writes `<stem>.png` (6× enlarged, old | new) into `out_dir`.
pub fn compare(path: &Path, out_dir: &Path) -> Result<(), String> {
    let img = decode(path, None, ColorTarget::Srgb).map_err(|e| format!("{}: {e}", path.display()))?;
    let size = (PORTRAIT_SIZE as f64 * PORTRAIT_CELL).round() as u32;
    let region = (PORTRAIT_ORIGIN.0.round() as u32, PORTRAIT_ORIGIN.1.round() as u32, size, size);

    let pal = pixelize::palette(&pixelize::colors(&img, region), PORTRAIT_COLORS);
    let mut old = pixelize::pixelize(&img, region, PORTRAIT_SIZE, PORTRAIT_SIZE, &pal);
    pixelize::cut_background(&mut old, BACKGROUND_TOLERANCE);

    let (cells, n, f) = portrait_cells(&img);
    let new = portrait_from_cells(&cells, n, f, &pixelize::palette(&cells, PORTRAIT_COLORS));

    // old | new, 6× each, on a neutral grey so the transparent background shows.
    let (a, b) = (upscale(&to_pixels(&old), 6), upscale(&to_pixels(&new), 6));
    let gap = 24;
    let (w, h) = (a.width + gap + b.width, a.height);
    let mut rgba = [90u8, 90, 96, 255].repeat((w * h) as usize);
    for (src, x0) in [(&a, 0), (&b, a.width + gap)] {
        for y in 0..h {
            for x in 0..src.width {
                let s = ((y * src.width + x) * 4) as usize;
                if src.rgba[s + 3] != 0 {
                    let d = ((y * w + x0 + x) * 4) as usize;
                    rgba[d..d + 4].copy_from_slice(&src.rgba[s..s + 4]);
                }
            }
        }
    }
    let stem = path.file_stem().unwrap().to_string_lossy();
    write_png(&out_dir.join(format!("{stem}.png")), &Pixels { width: w, height: h, rgba, icc: None });
    println!("  {stem}: {f} Gemini pixel(s) per portrait pixel");
    Ok(())
}

/// Where `to-png` puts a sprite for painting (not committed: the `.sprite` stays the source).
fn edit_png(paths: &Paths, name: &str) -> PathBuf {
    paths.out.join("edit").join(format!("{name}.png"))
}

fn read_sprite(paths: &Paths, name: &str) -> Result<(PathBuf, String), String> {
    let src = paths.src().join(format!("{name}.sprite"));
    let text = std::fs::read_to_string(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    Ok((src, text))
}

/// `.sprite` → PNG sheet for a pixel editor. Refuses to replace a sheet with unapplied painting.
pub fn sprite_to_png(paths: &Paths, name: &str, force: bool) -> Result<PathBuf, String> {
    let (src, text) = read_sprite(paths, name)?;
    let (sheet, _) = sprite::sheet(&sprite::parse(&text).map_err(|e| format!("{}: {e}", src.display()))?);
    let out = edit_png(paths, name);
    if out.exists() && !force && read_png(&out)?.rgba != sheet.rgba {
        return Err(format!(
            "{} contains painting that isn't in {name}.sprite yet.\nRun `from-png {name}` first, or `to-png {name} --force` to throw it away.",
            out.display()
        ));
    }
    write_png(&out, &sheet);
    Ok(out)
}

/// Edited PNG sheet → back into the `.sprite`.
pub fn sprite_from_png(paths: &Paths, name: &str) -> Result<sprite::Applied, String> {
    let (src, text) = read_sprite(paths, name)?;
    let img = read_png(&edit_png(paths, name))?;
    let applied = sprite::apply_sheet(&text, &img).map_err(|e| format!("{}: {e}", src.display()))?;
    if applied.text != text {
        std::fs::write(&src, &applied.text).map_err(|e| format!("{}: {e}", src.display()))?;
    }
    Ok(applied)
}

#[derive(Serialize, Clone, Debug)]
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

/// What differs per character; the app picks its own (`characters[key]`).
#[derive(Serialize)]
pub struct CharacterArt {
    /// The office: the character's own `office.png`, or the shared one.
    pub background: String,
    /// The animated sprite at the desk.
    pub desk: SpriteManifest,
    /// Mood → portrait, every mood filled (stand-ins for missing ones).
    pub portraits: BTreeMap<String, String>,
}

/// Everything the app needs to assemble the scene, in scene pixels (320×180).
#[derive(Serialize)]
pub struct Manifest {
    pub width: u32,
    pub height: u32,
    pub characters: BTreeMap<String, CharacterArt>,
    pub ui: SpriteManifest,
    /// Wilma, the office cat: `origin` is her cushion (bottom centre).
    pub wilma: SpriteManifest,
    /// The strip of floor in front of the desk where she wanders: x from–to, and the y of her paws.
    pub wilma_floor: [u32; 3],
    /// Dialogue mood → portraits to pick from.
    pub moods: BTreeMap<String, Vec<String>>,
    pub hotspots: BTreeMap<String, Hotspot>,
    /// Where Wolfgang's speech bubbles may go, and the point their tail aims at (his mouth).
    pub bubble_area: Rect,
    pub speaker: [u32; 2],
}

/// Mood → portrait for a character: their own, else a stand-in of theirs (`STAND_INS`,
/// then any), else Wolfgang's.
fn character_portraits(paths: &Paths, key: &str) -> BTreeMap<String, String> {
    let own: Vec<&str> = MOODS.iter().copied().filter(|m| paths.portrait(key, m).exists()).collect();
    MOODS
        .iter()
        .map(|mood| {
            let stand_ins = STAND_INS.iter().find(|(m, _)| m == mood).map(|(_, s)| *s).unwrap_or(&[]);
            let pick = std::iter::once(*mood).chain(stand_ins.iter().copied()).chain(own.iter().copied()).find(|m| own.contains(m));
            let path = match pick {
                Some(m) => paths.portrait(key, m),
                None => paths.portrait("wolfgang", mood),
            };
            (mood.to_string(), rel(paths, &path))
        })
        .collect()
}

/// FNV-1a: a stable fingerprint for `.desk-sync` (std's hasher may change between Rust versions).
fn fingerprint(data: &[u8]) -> String {
    let h = data.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    format!("{h:016x}")
}

/// A character's `desk.png` (painted) and `desk.sprite` (the text animation is written in)
/// are one sprite: whichever changed since the last build wins and the other is rewritten.
/// `.desk-sync` remembers both as of the last build; if both changed, nothing is touched and
/// the error says how to pick one. A `desk.png` without text gets Wolfgang's frame layout
/// and animations. No desk at all: `None` (the build uses Wolfgang's).
pub fn sync_desk(paths: &Paths, key: &str) -> Result<Option<SpriteManifest>, String> {
    let dir = paths.character(key);
    let (png_path, text_path, state_path) = (dir.join("desk.png"), dir.join("desk.sprite"), dir.join(".desk-sync"));
    let png = if png_path.exists() { Some(read_png(&png_path)?) } else { None };
    let text = if text_path.exists() { Some(std::fs::read_to_string(&text_path).map_err(|e| format!("{}: {e}", text_path.display()))?) } else { None };
    let show = |p: &Path| rel(paths, p);
    // Saving noise (Pixilart & co.) off the painting first, against the sprite's own colours.
    let png = png.map(|mut img| {
        let mut pal: Vec<pixelize::Rgb> = text.as_deref().and_then(|t| sprite::parse(t).ok()).map(|f| f.palette.values().copied().collect()).unwrap_or_default();
        let before = img.rgba.clone();
        snap(&mut img, &mut pal);
        if img.rgba != before {
            write_png(&png_path, &img);
            println!("  {}: colours snapped onto the sprite's palette", show(&png_path));
        }
        img
    });
    let apply = |text: &str, png: &Pixels| -> Result<String, String> {
        let a = sprite::apply_sheet(text, png).map_err(|e| format!("{}: {e}", show(&png_path)))?;
        println!("  {}: {} pixels painted → {}", show(&png_path), a.changed, show(&text_path));
        for (ch, [r, g, b]) in &a.new_colors {
            println!("    new colour '{ch}' {r:02x}{g:02x}{b:02x}");
        }
        for w in &a.warnings {
            println!("    ⚠ {w}");
        }
        Ok(a.text)
    };
    let text = match (text, png) {
        (None, None) => return Ok(None),
        (None, Some(png)) => {
            let layout = std::fs::read_to_string(paths.character("wolfgang").join("desk.sprite")).map_err(|e| format!("Wolfgang's desk.sprite: {e}"))?;
            apply(&sprite::blank(&layout)?, &png)?
        }
        (Some(text), None) => text,
        (Some(text), Some(png)) => {
            let file = sprite::parse(&text).map_err(|e| format!("{}: {e}", show(&text_path)))?;
            if sprite::sheet(&file).0.rgba == png.rgba {
                text
            } else {
                let state = std::fs::read_to_string(&state_path).unwrap_or_default();
                let was = |what: &str| state.lines().find_map(|l| l.strip_prefix(what)).map(str::trim).map(String::from);
                let text_changed = was("sprite ") != Some(fingerprint(text.as_bytes()));
                let png_changed = was("png ") != Some(fingerprint(&png.rgba));
                match (text_changed, png_changed) {
                    (false, true) => apply(&text, &png)?,
                    (true, false) => {
                        println!("  {} → {}", show(&text_path), show(&png_path));
                        text
                    }
                    _ => {
                        return Err(format!(
                            "{} and {} both changed since the last build and differ.\n\
                             Keep the painting:  cargo run --release -p wolfgang-art -- keep-png {key}\n\
                             Keep the text:      cargo run --release -p wolfgang-art -- keep-text {key}",
                            show(&png_path),
                            show(&text_path)
                        ))
                    }
                }
            }
        }
    };
    let file = sprite::parse(&text).map_err(|e| format!("{}: {e}", show(&text_path)))?;
    let (sheet, frames) = sprite::sheet(&file);
    if std::fs::read_to_string(&text_path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&text_path, &text).map_err(|e| format!("{}: {e}", text_path.display()))?;
    }
    if read_png(&png_path).map(|p| p.rgba != sheet.rgba).unwrap_or(true) {
        write_png(&png_path, &sheet);
    }
    let state = format!("# {key}'s desk.png and desk.sprite as of the last build (wolfgang-art)\nsprite {}\npng {}\n", fingerprint(text.as_bytes()), fingerprint(&sheet.rgba));
    if std::fs::read_to_string(&state_path).ok().as_deref() != Some(state.as_str()) {
        std::fs::write(&state_path, state).map_err(|e| format!("{}: {e}", state_path.display()))?;
    }
    Ok(Some(SpriteManifest { sheet: rel(paths, &png_path), origin: file.origin.map(|(x, y)| [x, y]), frames, animations: file.anims }))
}

/// Settles a conflict from [`sync_desk`]: the side that is kept overwrites the other.
pub fn keep_desk(paths: &Paths, key: &str, keep_png: bool) -> Result<(), String> {
    let dir = paths.character(key);
    if keep_png {
        std::fs::write(dir.join(".desk-sync"), format!("sprite {}\n", fingerprint(&std::fs::read(dir.join("desk.sprite")).map_err(|e| e.to_string())?)))
    } else {
        std::fs::remove_file(dir.join("desk.png"))
    }
    .map_err(|e| e.to_string())?;
    sync_desk(paths, key).map(|_| ())
}

/// Every character's art; Wolfgang's stands in for what someone doesn't have yet.
pub fn characters(paths: &Paths) -> BTreeMap<String, CharacterArt> {
    let wolfgang = sync_desk(paths, "wolfgang").unwrap_or_else(|e| panic!("{e}")).expect("Wolfgang's desk sprite");
    let mut out = BTreeMap::new();
    for c in wolfgang_core::character::all() {
        let desk = if c.key == "wolfgang" { None } else { sync_desk(paths, &c.key).unwrap_or_else(|e| panic!("{e}")) };
        let office = paths.character(&c.key).join("office.png");
        let portraits = character_portraits(paths, &c.key);
        let own = MOODS.iter().filter(|m| paths.portrait(&c.key, m).exists()).count();
        println!(
            "  {}: {own}/8 portraits{}, desk {}, office {}",
            c.key,
            if own < 8 { " (stand-ins for the rest)" } else { "" },
            if c.key == "wolfgang" || desk.is_some() { "own" } else { "Wolfgang's" },
            if office.exists() { "own" } else { "shared" }
        );
        let desk = desk.unwrap_or_else(|| wolfgang.clone());
        out.insert(c.key.clone(), CharacterArt { background: if office.exists() { rel(paths, &office) } else { rel(paths, &paths.office()) }, desk, portraits });
    }
    out
}

pub fn manifest(paths: &Paths, characters: BTreeMap<String, CharacterArt>, ui: SpriteManifest, wilma: SpriteManifest) {
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
        ("wolfgang", Hotspot { action: "about", label: "{name}: Über {name}", rects: vec![r(145, 52, 36, 46)] }),
        ("computer", Hotspot { action: "settings", label: "Computer: Einstellungen", rects: vec![r(207, 76, 38, 43)] }),
        ("clipboard", Hotspot { action: "presets", label: "Klemmbrett: Vorlagen", rects: vec![r(110, 106, 23, 15)] }),
        ("mug", Hotspot { action: "sip", label: "{name}s Kaffee", rects: vec![r(100, 110, 9, 11)] }),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    let m = Manifest {
        width: SCENE_W,
        height: SCENE_H,
        characters,
        ui,
        wilma,
        wilma_floor: [116, 232, 178],
        moods,
        hotspots,
        bubble_area: r(56, 4, 208, 46),
        speaker: [162, 70],
    };
    let path = paths.out.join("scene.json");
    let json = serde_json::to_string_pretty(&m).unwrap();
    std::fs::write(&path, json.clone() + "\n").unwrap();
    println!("  scene.json");
    for key in m.characters.keys() {
        preview(paths, &m, key);
    }
}

/// A self-contained HTML page (images inlined) to look at a character's scene in a browser.
fn preview(paths: &Paths, m: &Manifest, key: &str) {
    // The page knows one character: theirs, under the names the app gives it too.
    let art = &m.characters[key];
    let mut scene = serde_json::to_value(m).unwrap();
    scene["background"] = art.background.clone().into();
    scene["wolfgang"] = serde_json::to_value(&art.desk).unwrap();
    scene["portraits"] = serde_json::to_value(&art.portraits).unwrap();
    let scene_json = serde_json::to_string(&scene).unwrap();
    let mut files: Vec<String> = vec![art.background.clone(), art.desk.sheet.clone(), m.ui.sheet.clone(), m.wilma.sheet.clone()];
    files.extend(art.portraits.values().cloned());
    files.sort();
    files.dedup();
    let images: BTreeMap<String, String> = files
        .into_iter()
        .map(|f| {
            let bytes = std::fs::read(paths.out.join(&f)).unwrap();
            (f, format!("data:image/png;base64,{}", base64(&bytes)))
        })
        .collect();
    let html = include_str!("preview.html")
        .replace("/*SCENE*/{}", &scene_json)
        .replace("/*IMAGES*/{}", &serde_json::to_string(&images).unwrap());
    let path = PathBuf::from(if key == "wolfgang" { "target/wolfgang-art/preview.html".to_string() } else { format!("target/wolfgang-art/preview-{key}.html") });
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, html).unwrap();
    println!("  preview: {}", std::fs::canonicalize(&path).unwrap().display());
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
