//! Runs the real `wolfgang` binary (non-interactive: tests have no terminal).

use std::path::Path;
use std::process::{Command, Output};
use wolfgang_core::image::{self, Pixels};
use wolfgang_core::ImageFormat;

fn jpg(path: &Path, shade: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let px = Pixels { width: 900, height: 600, rgba: vec![shade; 900 * 600 * 4], icc: None };
    std::fs::write(path, image::encode(&px, ImageFormat::Jpg, 80.0).unwrap()).unwrap();
}

fn wolfgang(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wolfgang")).args(args).env("WOLFGANG_HOME", home).env("NO_COLOR", "1").output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn converts_with_flags_and_remembers_last_run() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Urlaub");
    jpg(&root.join("a.jpg"), 10);
    jpg(&root.join("b.jpg"), 20);
    let home = t.path().join("home");

    let o = wolfgang(&home, &["--yes", "-f", "avif", "-d", "450", "-n", "Strand Tag", "-k", "Meer", root.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("Fertig!"), "{}", text(&o));
    let out = t.path().join("Urlaub/wolfgang_output");
    assert!(out.join("strand-tag-meer-0001.avif").exists() && out.join("strand-tag-meer-0002.avif").exists());

    let settings = std::fs::read_to_string(home.join("settings.json")).unwrap();
    assert!(settings.contains("\"last_run\"") && settings.contains("450"), "{settings}");

    // Second run: nothing new.
    let o = wolfgang(&home, &["-y", root.to_str().unwrap()]);
    assert!(o.status.success());
    assert!(text(&o).contains("schon alles fertig"), "{}", text(&o));
}

#[test]
fn json_events() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("J");
    jpg(&root.join("a.jpg"), 30);
    std::fs::write(root.join("broken.jpg"), "nope").unwrap();
    let o = wolfgang(&t.path().join("home"), &["--json", "--hq", "-f", "png", root.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "a failed file means exit 1");
    let events: Vec<serde_json::Value> = String::from_utf8(o.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(events.first().unwrap()["event"], "plan");
    assert_eq!(events.first().unwrap()["jobs"], 2);
    let files: Vec<_> = events.iter().filter(|e| e["event"] == "file").collect();
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|f| f["ok"] == true && f["target"] == "a.png"));
    assert!(files.iter().any(|f| f["ok"] == false && f["error"].as_str().is_some()));
    let done = events.last().unwrap();
    assert_eq!((done["event"].as_str(), done["converted"].as_u64(), done["failed"].as_u64()), (Some("done"), Some(1), Some(1)));
}

#[test]
fn usage_errors_exit_2() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let dir = t.path().to_str().unwrap();
    for args in [
        vec!["-y", "--hq", "--size", "800", dir],
        vec!["-y", "-f", "jpg", "-q", "50", dir],
        vec!["-y", "--web", "--hq", dir],
        vec!["-y", "-p", "gibtsnicht", dir],
        vec!["-y", "/gibt/es/nicht"],
        vec!["-y"],
        vec!["-y", "-d", "5", dir],
    ] {
        let o = wolfgang(&home, &args);
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", text(&o));
    }
}

#[test]
fn only_fonts_leaves_images_alone() {
    let font = Path::new("/System/Library/Fonts/Supplemental/Arial.ttf");
    if !font.exists() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Mix");
    jpg(&root.join("a.jpg"), 40);
    std::fs::copy(font, root.join("Arial.ttf")).unwrap();
    let o = wolfgang(&t.path().join("home"), &["-y", "--only", "fonts", root.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let out = t.path().join("Mix/wolfgang_output");
    assert!(out.join("fonts/arial-400.woff2").exists());
    assert!(!out.join("a.webp").exists() && std::fs::read_dir(&out).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "webp")).count() == 0);
}

#[test]
fn settings_and_presets_commands() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    assert!(wolfgang(&home, &["settings", "set", "web-format", "avif"]).status.success());
    assert!(wolfgang(&home, &["settings", "set", "quality", "70"]).status.success());
    assert_eq!(wolfgang(&home, &["settings", "set", "quality", "700"]).status.code(), Some(2));
    assert!(wolfgang(&home, &["settings", "set", "video-format", "webm"]).status.success());
    assert!(wolfgang(&home, &["settings", "set", "video-size", "original"]).status.success());
    assert!(wolfgang(&home, &["settings", "set", "audio-format", "aac"]).status.success());
    assert_eq!(wolfgang(&home, &["settings", "set", "video-size", "9"]).status.code(), Some(2));
    let show = text(&wolfgang(&home, &["settings"]));
    assert!(show.contains("AVIF") && show.contains("quality        70"), "{show}");
    assert!(show.contains("video-format   WebM") && show.contains("video-size     original") && show.contains("audio-format   AAC"), "{show}");

    // Defaults are used by the next run.
    let root = t.path().join("D");
    jpg(&root.join("a.jpg"), 50);
    assert!(wolfgang(&home, &["-y", root.to_str().unwrap()]).status.success());
    assert!(t.path().join("D/wolfgang_output/a-0001.avif").exists());

    assert!(text(&wolfgang(&home, &["presets"])).contains("Noch keine Vorlagen"));
    assert_eq!(wolfgang(&home, &["presets", "remove", "x"]).status.code(), Some(2));
}

#[test]
fn legacy_reset_renames_only_logged_files() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("Alt");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("DSC_1-0001.jpg"), "x").unwrap();
    std::fs::write(d.join("Sommer-2023.jpg"), "x").unwrap();
    let log = serde_json::to_string_pretty(&serde_json::json!({
        "converted_files": [{ "original_path": d.join("DSC_1-0001.jpg"), "original_relative_path": "DSC_1-0001.jpg", "filename_index": "0001" }]
    }))
    .unwrap();
    std::fs::write(d.join(".wolfgang_run_log.jsonl"), log).unwrap();
    let home = t.path().join("home");

    // Without a terminal and without --yes: show only.
    let o = wolfgang(&home, &["legacy-reset", d.to_str().unwrap()]);
    assert!(o.status.success() && text(&o).contains("Hab nix angefasst"), "{}", text(&o));
    assert!(d.join("DSC_1-0001.jpg").exists());

    // Old muscle memory: -r PATH -y
    let o = wolfgang(&home, &["-r", "-y", d.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(d.join("DSC_1.jpg").exists() && d.join("Sommer-2023.jpg").exists());
    assert!(!d.join(".wolfgang_run_log.jsonl").exists() && d.join(".wolfgang_run_log.jsonl.done").exists());
}

#[test]
fn videos_and_audio_without_questions() {
    let Some(tools) = wolfgang_core::media::tools() else {
        eprintln!("skipped: no ffmpeg in target/ffmpeg (scripts/build-ffmpeg.sh)");
        return;
    };
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Messe");
    jpg(&root.join("stand.jpg"), 40);
    std::fs::create_dir_all(&root).unwrap();
    let make = |args: &[&str], out: &str| {
        let ok = Command::new(&tools.ffmpeg).args(["-v", "error", "-y"]).args(args).arg(root.join(out)).status().unwrap();
        assert!(ok.success(), "{out}");
    };
    make(&["-f", "lavfi", "-i", "testsrc2=s=1280x720:d=1", "-f", "lavfi", "-i", "sine=d=1", "-c:v", "ffv1", "-c:a", "pcm_s16le", "-f", "matroska"], "Rundgang Halle 3.mkv");
    make(&["-f", "lavfi", "-i", "sine=d=1", "-c:a", "pcm_s16le", "-f", "wav"], "Begrüßung.wav");
    let home = t.path().join("home");

    let o = wolfgang(&home, &["--yes", "--only", "videos", "--video-size", "original", "--no-poster", root.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    let out = t.path().join("Messe/wolfgang_output");
    assert!(out.join("rundgang-halle-3.mp4").exists(), "{}", text(&o));
    assert!(!out.join("rundgang-halle-3.webp").exists() && !out.join("stand.webp").exists() && !out.join("begruessung.mp3").exists());

    let o = wolfgang(&home, &["--yes", "--audio", "speech", root.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(out.join("begruessung.mp3").exists() && out.join("stand-0001.webp").exists(), "{}", text(&o));

    let settings = std::fs::read_to_string(home.join("settings.json")).unwrap();
    assert!(settings.contains("\"videos\"") && settings.contains("\"speech\""), "remembered per pile: {settings}");
}
