//! End-to-end for videos and audio with the bundled ffmpeg: generated clips in,
//! real files out, checked with ffprobe. Skipped (with a note) if ffmpeg isn't
//! built; CI builds it with `scripts/build-ffmpeg.sh`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use wolfgang_core::image::{self, ColorTarget, Pixels};
use wolfgang_core::media::{self, MediaInfo};
use wolfgang_core::run::Event;
use wolfgang_core::runlog::RunLog;
use wolfgang_core::*;

fn tools() -> Option<&'static media::Tools> {
    let t = media::tools();
    if t.is_none() {
        eprintln!("skipped: no ffmpeg in target/ffmpeg (scripts/build-ffmpeg.sh)");
    }
    t
}

/// Makes a test clip with ffmpeg's built-in generators (lossless FFV1/PCM in MKV).
fn clip(path: &Path, size: &str, fps: u32, secs: f32, sound: bool, extra_vf: Option<&str>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let t = media::tools().unwrap();
    let mut cmd = Command::new(&t.ffmpeg);
    cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("testsrc2=s={size}:r={fps}:d={secs}")]);
    if sound {
        cmd.args(["-f", "lavfi", "-i", &format!("sine=f=440:d={secs}")]);
    }
    if let Some(vf) = extra_vf {
        cmd.args(["-vf", vf]);
    }
    cmd.args(["-c:v", "ffv1"]);
    if sound {
        cmd.args(["-c:a", "pcm_s16le"]);
    }
    cmd.args(["-f", "matroska"]).arg(path);
    assert!(cmd.status().unwrap().success(), "making {}", path.display());
}

fn wav(path: &Path, rate: u32, secs: f32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let t = media::tools().unwrap();
    let ok = Command::new(&t.ffmpeg)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", &format!("sine=f=330:r={rate}:d={secs}"), "-ac", "2", "-c:a", "pcm_s16le", "-f", "wav"])
        .arg(path)
        .status()
        .unwrap();
    assert!(ok.success());
}

fn videos(v: VideoSettings) -> RunSettings {
    RunSettings { videos: Some(v), ..Default::default() }
}

fn run(dir: &Path, settings: &RunSettings) -> (Plan, Report) {
    let ws = Workspace::open(&[dir.to_path_buf()]).unwrap();
    let plan = ws.plan(settings, RerunPolicy::OnlyNew).unwrap();
    let report = execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    (plan, report)
}

fn probe(p: &Path) -> MediaInfo {
    media::probe(p).unwrap()
}

/// codec name of the first video/audio stream
fn codecs(p: &Path) -> (String, String) {
    let t = media::tools().unwrap();
    let out = |sel: &str| {
        let o = Command::new(&t.ffprobe)
            .args(["-v", "error", "-select_streams", sel, "-show_entries", "stream=codec_name,color_transfer", "-of", "csv=p=0"])
            .arg(p)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_owned()
    };
    (out("v:0"), out("a:0"))
}

#[test]
fn video_to_mp4_with_sound_and_poster() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Filme");
    clip(&dir.join("Drohne Strand FINAL.mkv"), "1920x1080", 30, 2.0, true, None);

    let settings = videos(VideoSettings { size: Some(720), ..Default::default() });
    let (plan, report) = run(&dir, &settings);
    let out = plan.output_root.join("drohne-strand-final.mp4");
    let info = probe(&out);
    let v = info.video.unwrap();
    assert_eq!((v.width, v.height), (1280, 720));
    assert!(!v.hdr);
    assert_eq!(info.audio.unwrap().channels, 2);
    assert_eq!(codecs(&out).0, "h264,bt709");
    assert_eq!(codecs(&out).1, "aac");
    assert!((info.duration.unwrap() - 2.0).abs() < 0.2, "{:?}", info.duration);

    let poster = plan.output_root.join("drohne-strand-final.webp");
    let px = image::decode(&poster, None, ColorTarget::Srgb).unwrap();
    assert_eq!((px.width, px.height), (1280, 720), "poster has the video's size");
    let bytes_out = std::fs::metadata(&out).unwrap().len() + std::fs::metadata(&poster).unwrap().len();
    assert_eq!(report.bytes_out, bytes_out, "the poster counts too");

    let log = RunLog::open(&plan.output_root).unwrap();
    let rec = log.files.values().next().unwrap();
    assert_eq!(rec.outputs, ["drohne-strand-final.mp4", "drohne-strand-final.webp"]);
    assert!(std::fs::read_dir(&plan.output_root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".part")));

    // Dropped again: already known.
    let ws = Workspace::open(&[dir]).unwrap();
    assert_eq!(ws.summary().videos.known, 1);
    assert!(ws.plan(&settings, RerunPolicy::OnlyNew).unwrap().jobs.is_empty());
}

#[test]
fn vertical_video_to_silent_webm_without_poster() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Hochkant");
    clip(&dir.join("story.mkv"), "1080x1920", 30, 1.0, true, None);

    let settings = videos(VideoSettings { format: VideoFormat::Webm, size: Some(720), sound: false, poster: false, ..Default::default() });
    let (plan, _) = run(&dir, &settings);
    let out = plan.output_root.join("story.webm");
    let info = probe(&out);
    let v = info.video.unwrap();
    assert_eq!((v.width, v.height), (720, 1280), "720p means the shorter side");
    assert!(info.audio.is_none(), "sound removed");
    assert_eq!(codecs(&out).0.split(',').next(), Some("vp9"));
    assert!(!plan.output_root.join("story.webp").exists());
}

#[test]
fn hdr_slow_motion_becomes_sdr_at_60fps_and_small_videos_stay_small() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("iPhone");
    // HLG like an iPhone, at 120 fps.
    let hlg = "zscale=tin=bt709:min=bt709:pin=bt709:t=arib-std-b67:m=bt2020nc:p=bt2020:npl=203,format=yuv420p10le,\
               setparams=color_trc=arib-std-b67:color_primaries=bt2020:colorspace=bt2020nc";
    clip(&dir.join("IMG_0001.mkv"), "640x360", 120, 1.0, false, Some(hlg));
    assert!(probe(&dir.join("IMG_0001.mkv")).video.unwrap().hdr, "test clip is HDR");

    let (plan, _) = run(&dir, &videos(VideoSettings::default()));
    let out = plan.output_root.join("img-0001.mp4");
    let v = probe(&out).video.unwrap();
    assert!(!v.hdr, "tone-mapped to SDR");
    assert_eq!((v.width, v.height), (640, 360), "never upscaled to 1080p");
    assert!(v.fps.unwrap() <= 60.5, "{:?}", v.fps);
    assert_eq!(codecs(&out).0, "h264,bt709");
}

#[test]
fn audio_to_mp3_and_aac() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Ton");
    wav(&dir.join("Interview Müller.wav"), 96_000, 1.5);

    let speech = RunSettings { audio: Some(AudioSettings { format: AudioFormat::Mp3, content: AudioContent::Speech }), ..Default::default() };
    let (plan, report) = run(&dir, &speech);
    let out = plan.output_root.join("interview-mueller.mp3");
    let a = probe(&out).audio.unwrap();
    assert_eq!((a.channels, a.sample_rate), (1, 48_000), "speech is mono, 96 kHz is reduced");
    assert!(report.bytes_out < report.bytes_in / 10, "{} → {}", report.bytes_in, report.bytes_out);

    std::fs::remove_dir_all(&plan.output_root).unwrap();
    let music = RunSettings { audio: Some(AudioSettings { format: AudioFormat::Aac, content: AudioContent::Music }), ..Default::default() };
    let (plan, _) = run(&dir, &music);
    let out = plan.output_root.join("interview-mueller.m4a");
    assert_eq!(probe(&out).audio.unwrap().channels, 2);
    assert_eq!(codecs(&out).1, "aac");
}

#[test]
fn names_dont_collide_with_photos_and_progress_is_reported() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Mix");
    clip(&dir.join("clip.mkv"), "320x240", 25, 2.0, false, None);
    let px = Pixels { width: 64, height: 48, rgba: [200, 30, 30, 255].repeat(64 * 48), icc: None };
    std::fs::write(dir.join("clip.jpg"), image::encode(&px, ImageFormat::Jpg, 90.0).unwrap()).unwrap();

    let settings = RunSettings {
        images: Some(ImageSettings::HighQuality { format: ImageFormat::Webp }),
        videos: Some(VideoSettings::default()),
        ..Default::default()
    };
    let ws = Workspace::open(&[dir]).unwrap();
    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();
    let mut targets: Vec<String> = plan.jobs.iter().flat_map(|j| j.outputs()).map(|p| p.to_string_lossy().into_owned()).collect();
    targets.sort();
    assert_eq!(targets, ["clip-mkv.mp4", "clip-mkv.webp", "clip.webp"], "the poster doesn't overwrite the photo");

    let fractions = Mutex::new(Vec::new());
    let report = execute(
        &plan,
        &|e| {
            if let Event::FileProgress { fraction, .. } = e {
                fractions.lock().unwrap().push(fraction);
            }
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(report.converted, 2);
    let f = fractions.into_inner().unwrap();
    assert!(!f.is_empty() && f.windows(2).all(|w| w[0] <= w[1]) && *f.last().unwrap() == 1.0, "{f:?}");
}

#[test]
fn cancelling_stops_ffmpeg_right_away() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Lang");
    clip(&dir.join("lang.mkv"), "1280x720", 30, 20.0, false, None);
    let ws = Workspace::open(&[dir]).unwrap();
    let settings = videos(VideoSettings { format: VideoFormat::Webm, size: None, quality: VideoQuality::Nicer, ..Default::default() });
    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();

    let cancel = AtomicBool::new(false);
    let started = std::time::Instant::now();
    let report = execute(
        &plan,
        &|e| {
            if let Event::FileProgress { fraction, .. } = e {
                if fraction > 0.0 {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        },
        &cancel,
    )
    .unwrap();
    assert!(report.cancelled && report.converted == 0 && report.failed.is_empty(), "{report:?}");
    assert!(started.elapsed().as_secs() < 15, "didn't wait for the whole video");
    let left: Vec<PathBuf> = std::fs::read_dir(&plan.output_root).unwrap().map(|e| e.unwrap().path()).filter(|p| !p.ends_with(".wolfgang")).collect();
    assert!(left.is_empty(), "half-written files are removed: {left:?}");
}

#[test]
fn broken_video_fails_politely() {
    if tools().is_none() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Kaputt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kaputt.mp4"), b"not a video at all").unwrap();
    let ws = Workspace::open(&[dir]).unwrap();
    assert_eq!(ws.summary().videos_with_sound, 1, "unknown sound: Wolfgang asks anyway");
    let plan = ws.plan(&videos(VideoSettings::default()), RerunPolicy::OnlyNew).unwrap();
    let report = execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].1.contains("kaputt.mp4"), "{:?}", report.failed);
}
