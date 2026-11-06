//! End-to-end: real folders in, real files out.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use wolfgang_core::image::{self, ColorTarget, Pixels};
use wolfgang_core::naming::Separator;
use wolfgang_core::run::Event;
use wolfgang_core::runlog::RunLog;
use wolfgang_core::*;

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
const DISPLAY_P3: &str = "/System/Library/ColorSync/Profiles/Display P3.icc";

fn pixels(w: u32, h: u32, alpha: bool) -> Pixels {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let a = if alpha && x < w / 4 { 0 } else { 255 };
            rgba.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 90, a]);
        }
    }
    Pixels { width: w, height: h, rgba, icc: None }
}

fn write_image(path: &Path, p: &Pixels, format: ImageFormat) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, image::encode(p, format, 90.0).unwrap()).unwrap();
}

fn dims(path: &Path) -> (u32, u32) {
    let p = image::decode(path, None, ColorTarget::Srgb).unwrap();
    (p.width, p.height)
}

fn files_in(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = walkdir(root)
        .into_iter()
        .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
        .filter(|p| !p.starts_with(".wolfgang"))
        .collect();
    v.sort();
    v
}

fn walkdir(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn web(prefix: &str, format: WebFormat, max_side: u32) -> RunSettings {
    RunSettings {
        images: Some(ImageSettings::Web(WebSettings {
            format,
            max_side,
            quality: 80,
            naming: Naming { prefix: prefix.into(), use_keyword_files: true, keywords_all: "Sommer".into(), ..Default::default() },
        })),
        fonts: Some(FontSettings {}),
    }
}

fn run(plan: &Plan) -> Report {
    execute(plan, &|_| {}, &AtomicBool::new(false)).unwrap()
}

/// Folder: Urlaub/{a.jpg, b.png (alpha), Strand/{c.jpg, keywords.txt}, Fonts/Arial.ttf, notes.pdf}
fn holiday_folder(t: &Path) -> PathBuf {
    let root = t.join("Urlaub");
    write_image(&root.join("a.jpg"), &pixels(3000, 2000, false), ImageFormat::Jpg);
    write_image(&root.join("b.png"), &pixels(600, 900, true), ImageFormat::Png);
    write_image(&root.join("Strand/c.jpg"), &pixels(1600, 1600, false), ImageFormat::Jpg);
    std::fs::write(root.join("Strand/keywords.txt"), "Strand Sonnenuntergang\nAlgarve\n").unwrap();
    if Path::new(ARIAL).exists() {
        std::fs::create_dir_all(root.join("Fonts")).unwrap();
        std::fs::copy(ARIAL, root.join("Fonts/Arial.ttf")).unwrap();
    }
    std::fs::write(root.join("notes.pdf"), "%PDF").unwrap();
    root
}

#[test]
fn web_run_end_to_end() {
    let t = tempfile::tempdir().unwrap();
    let root = holiday_folder(t.path());
    let has_font = Path::new(ARIAL).exists();

    let ws = Workspace::open(std::slice::from_ref(&root)).unwrap();
    assert_eq!(ws.output_root, t.path().canonicalize().unwrap().join("Urlaub-wolfgang"));
    let s = ws.summary();
    assert_eq!((s.images.total, s.images.known), (3, 0));
    assert_eq!(s.fonts.total, has_font as usize);
    assert_eq!(s.unknown, vec![PathBuf::from("notes.pdf")]);
    assert_eq!(s.folders_without_keywords, vec![""]);
    assert_eq!(s.keyword_files.iter().map(|k| k.0.as_str()).collect::<Vec<_>>(), vec!["Strand"]);

    let plan = ws.plan(&web("Urlaub", WebFormat::Webp, 1400), RerunPolicy::OnlyNew).unwrap();
    let events = Mutex::new(Vec::new());
    let report = execute(&plan, &|e| events.lock().unwrap().push(e), &AtomicBool::new(false)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(report.converted, 3 + has_font as usize);
    assert!(report.bytes_out < report.bytes_in);

    let mut expected = vec!["Strand/urlaub-strand-sonnenuntergang-algarve-0001.webp", "urlaub-sommer-0001.webp", "urlaub-sommer-0002.webp"];
    if has_font {
        expected.insert(1, "fonts/Fonts/arial-400.woff2");
    }
    assert_eq!(files_in(&ws.output_root), expected);

    let out = &ws.output_root;
    assert_eq!(dims(&out.join("urlaub-sommer-0001.webp")), (1400, 933), "landscape scaled to longest side");
    assert_eq!(dims(&out.join("urlaub-sommer-0002.webp")), (600, 900), "never upscaled");
    let webp = std::fs::read(out.join("urlaub-sommer-0002.webp")).unwrap();
    assert!(!webp.windows(4).any(|w| w == b"ICCP" || w == b"EXIF"), "web output carries no metadata");

    let events = events.into_inner().unwrap();
    assert!(matches!(events[0], Event::Started { total, .. } if total == report.converted));
    assert_eq!(events.iter().filter(|e| matches!(e, Event::FileDone(_))).count(), report.converted);

    // Originals untouched, no stray temp files.
    assert!(root.join("a.jpg").exists());
    assert!(!walkdir(out).iter().any(|p| p.to_string_lossy().ends_with(".part")));
}

#[test]
fn rerun_skips_known_and_continues_index() {
    let t = tempfile::tempdir().unwrap();
    let root = holiday_folder(t.path());
    let settings = web("", WebFormat::Avif, 800);

    let ws = Workspace::open(std::slice::from_ref(&root)).unwrap();
    run(&ws.plan(&settings, RerunPolicy::OnlyNew).unwrap());

    write_image(&root.join("d.jpg"), &pixels(1000, 500, false), ImageFormat::Jpg);
    let ws = Workspace::open(std::slice::from_ref(&root)).unwrap();
    assert!(ws.output_root.ends_with("Urlaub-wolfgang"), "same folder found again");
    let s = ws.summary();
    assert_eq!((s.images.total, s.images.known, s.images.fresh()), (4, 3, 1));
    assert_eq!(s.previous_runs, 1);

    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();
    let targets: Vec<_> = plan.jobs.iter().map(|j| j.target.to_string_lossy().into_owned()).collect();
    assert_eq!(targets, vec!["sommer-0003.avif"]);
    run(&plan);

    // "Alles nochmal": every file keeps its index, so names stay stable.
    let ws = Workspace::open(&[root]).unwrap();
    let plan = ws.plan(&settings, RerunPolicy::All).unwrap();
    let mut targets: Vec<_> = plan.jobs.iter().filter(|j| matches!(j.task, workspace::Task::Image(_))).map(|j| j.target.to_string_lossy().into_owned()).collect();
    targets.sort();
    assert_eq!(targets, vec!["Strand/strand-sonnenuntergang-algarve-0001.avif", "sommer-0001.avif", "sommer-0002.avif", "sommer-0003.avif"]);
    run(&plan);
    assert_eq!(RunLog::open(&ws.output_root).unwrap().runs, 3);
}

#[test]
fn high_quality_keeps_names_size_and_resolves_collisions() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Scans");
    write_image(&root.join("Foto.jpg"), &pixels(2600, 1200, false), ImageFormat::Jpg);
    write_image(&root.join("Foto.png"), &pixels(300, 200, true), ImageFormat::Png);

    let ws = Workspace::open(&[root]).unwrap();
    let settings = RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Webp }), fonts: None };
    let report = run(&ws.plan(&settings, RerunPolicy::OnlyNew).unwrap());
    assert_eq!(report.converted, 2);
    assert_eq!(files_in(&ws.output_root), vec!["Foto-png.webp", "Foto.webp"]);
    assert_eq!(dims(&ws.output_root.join("Foto.webp")), (2600, 1200));
}

#[test]
fn high_quality_keeps_wide_gamut_profiles() {
    let Ok(p3) = std::fs::read(DISPLAY_P3) else { return };
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("p3.png");
    let mut px = pixels(64, 64, false);
    px.icc = Some(p3.clone());
    write_image(&src, &px, ImageFormat::Png);

    let kept = image::decode(&src, None, ColorTarget::KeepSource).unwrap();
    assert!(kept.icc.as_ref().is_some_and(|i| i.len() > 100), "P3 profile kept");
    assert!(image::decode(&src, None, ColorTarget::Srgb).unwrap().icc.is_none());

    for format in [ImageFormat::Jpg, ImageFormat::Webp, ImageFormat::Png] {
        let task = image::ImageTask { format, max_side: None, quality: 95.0, color: ColorTarget::KeepSource };
        let out = t.path().join(format!("out.{}", format.ext()));
        std::fs::write(&out, image::convert(&src, &task).unwrap()).unwrap();
        assert!(image::decode(&out, None, ColorTarget::KeepSource).unwrap().icc.is_some(), "{format:?} lost the profile");
    }
}

#[test]
fn heic_input_via_imageio() {
    // sips ships with macOS; use it to make a HEIC fixture.
    let t = tempfile::tempdir().unwrap();
    let jpg = t.path().join("src.jpg");
    write_image(&jpg, &pixels(800, 600, false), ImageFormat::Jpg);
    let heic = t.path().join("Album/photo.heic");
    std::fs::create_dir_all(heic.parent().unwrap()).unwrap();
    let ok = std::process::Command::new("sips").args(["-s", "format", "heic"]).arg(&jpg).arg("--out").arg(&heic).output().is_ok_and(|o| o.status.success());
    if !ok {
        return;
    }
    let ws = Workspace::open(&[heic.parent().unwrap().to_path_buf()]).unwrap();
    let report = run(&ws.plan(&web("", WebFormat::Webp, 400), RerunPolicy::OnlyNew).unwrap());
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(dims(&ws.output_root.join("sommer-0001.webp")), (400, 300));
}

#[test]
fn output_folder_of_another_source_is_not_reused() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("one/Bilder");
    write_image(&a.join("x.jpg"), &pixels(50, 50, false), ImageFormat::Jpg);
    // Someone already has a "Bilder-wolfgang" folder next to it that isn't ours.
    std::fs::create_dir_all(t.path().join("one/Bilder-wolfgang")).unwrap();
    std::fs::write(t.path().join("one/Bilder-wolfgang/private.txt"), "nicht anfassen").unwrap();

    let ws = Workspace::open(&[a]).unwrap();
    assert!(ws.output_root.ends_with("Bilder-wolfgang-2"), "{}", ws.output_root.display());
}

#[test]
fn loose_files_go_next_to_their_folder_and_duplicates_are_skipped() {
    let t = tempfile::tempdir().unwrap();
    let desk = t.path().join("Schreibtisch");
    let px = pixels(100, 80, false);
    write_image(&desk.join("one.jpg"), &px, ImageFormat::Jpg);
    std::fs::copy(desk.join("one.jpg"), desk.join("copy of one.jpg")).unwrap();

    let ws = Workspace::open(&[desk.join("one.jpg"), desk.join("copy of one.jpg")]).unwrap();
    assert!(ws.output_root.ends_with("Schreibtisch/Schreibtisch-wolfgang"));
    assert_eq!(ws.summary().duplicates, 1);
    let plan = ws.plan(&web("x", WebFormat::Webp, 1400), RerunPolicy::OnlyNew).unwrap();
    assert_eq!(plan.jobs.len(), 1);
    assert_eq!(plan.skipped_duplicates.len(), 1);
}

#[test]
fn broken_files_fail_alone_and_cancel_stops() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Mix");
    write_image(&root.join("good.jpg"), &pixels(100, 100, false), ImageFormat::Jpg);
    std::fs::write(root.join("broken.jpg"), b"not really a jpeg").unwrap();
    std::fs::write(root.join("broken.ttf"), b"not a font").unwrap();

    let ws = Workspace::open(&[root]).unwrap();
    let plan = ws.plan(&web("", WebFormat::Webp, 1400), RerunPolicy::OnlyNew).unwrap();
    assert_eq!(plan.problems.len(), 1, "broken font is reported at planning time");

    let cancelled = execute(&plan, &|_| {}, &AtomicBool::new(true)).unwrap();
    assert!(cancelled.cancelled);
    assert_eq!(cancelled.converted, 0);

    let report = run(&plan);
    assert_eq!(report.converted, 1);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].0, PathBuf::from("broken.jpg"));
}

#[test]
fn caps_separator_and_typed_folder_keywords() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Shop");
    write_image(&root.join("Schuhe/1.jpg"), &pixels(64, 64, false), ImageFormat::Jpg);
    let mut naming = Naming { prefix: "Müller".into(), separator: Separator::Caps, append_original: true, ..Default::default() };
    naming.folder_keywords.insert("Schuhe".into(), "rote Schuhe\nLeder".into());
    let settings = RunSettings { images: Some(ImageSettings::Web(WebSettings { naming, ..Default::default() })), fonts: None };

    let ws = Workspace::open(&[root]).unwrap();
    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();
    assert_eq!(plan.jobs[0].target, PathBuf::from("Schuhe/MuellerRoteSchuheLeder10001.webp"));
}

#[test]
fn excluding_a_pile() {
    let t = tempfile::tempdir().unwrap();
    let root = holiday_folder(t.path());
    let mut ws = Workspace::open(&[root]).unwrap();
    ws.exclude(FileKind::Image);
    let s = ws.summary();
    assert_eq!(s.images.total, 0);
    assert!(s.folders_without_keywords.is_empty() && s.keyword_files.is_empty());
    assert_eq!(s.fonts.total, Path::new(ARIAL).exists() as usize);
    assert_eq!(s.unknown.len(), 1);
}
