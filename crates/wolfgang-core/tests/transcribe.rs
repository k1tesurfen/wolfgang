//! End-to-end for transcripts: German speech from macOS's own voice („Anna“) after
//! a stretch of music, as audio and inside a video, written down by Whisper.
//! Skipped (with a note) without ffmpeg (scripts/build-ffmpeg.sh) or without the
//! add-on's model files in `target/models` (copy them there from an installed
//! Wolfgang: ~/Library/Application Support/Wolfgang/models).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wolfgang_core::run::Event;
use wolfgang_core::*;

const SPEECH: &str = "Guten Tag zusammen. Heute besprechen wir das neue Projekt für den Kunden. Wir fangen im Januar an.";

fn ready() -> bool {
    let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/models");
    // Only this test binary reads the variable, before any transcript is made.
    std::env::set_var(transcribe::DIR_ENV, &models);
    match (media::tools(), transcribe::installed()) {
        (Some(_), true) => true,
        (None, _) => {
            eprintln!("skipped: no ffmpeg in target/ffmpeg (scripts/build-ffmpeg.sh)");
            false
        }
        (_, false) => {
            eprintln!("skipped: no model files in target/models");
            false
        }
    }
}

/// 8 s of a chord (the waiting music), then Anna speaking, as AAC; or the same as
/// a small H.264 video. `times` repeats the speech.
fn recording(dir: &Path, name: &str, times: usize) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let aiff = dir.join(format!("{name}.aiff"));
    let text = vec![SPEECH; times].join(" ");
    assert!(Command::new("say").args(["-v", "Anna", "-o"]).arg(&aiff).arg(&text).status().unwrap().success());
    let ffmpeg = &media::tools().unwrap().ffmpeg;
    let out = dir.join(name);
    let video = name.ends_with(".mp4");
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=f=440:d=8,volume=0.3", "-f", "lavfi", "-i", "sine=f=554:d=8,volume=0.3"]).arg("-i").arg(&aiff);
    let mix = "[0][1]amix=inputs=2[m];[m][2]concat=n=2:v=0:a=1,aresample=44100[a]";
    if video {
        cmd.args(["-f", "lavfi", "-i", "testsrc2=s=320x240:r=10"]).args(["-filter_complex", mix, "-map", "3:v", "-map", "[a]", "-shortest", "-c:v", "h264_videotoolbox", "-allow_sw", "1"]);
    } else {
        cmd.args(["-filter_complex", mix, "-map", "[a]"]);
    }
    assert!(cmd.args(["-c:a", "aac_at"]).arg(&out).status().unwrap().success(), "making {name}");
    std::fs::remove_file(aiff).unwrap();
    out
}

#[test]
fn meetings_are_written_down() {
    if !ready() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Besprechungen");
    recording(&dir, "Termin Kunde.m4a", 1);
    recording(&dir, "Videocall.mp4", 1);

    let ws = Workspace::open(std::slice::from_ref(&dir)).unwrap();
    let s = ws.summary();
    assert!(s.audio_facts[0].info.as_ref().and_then(|i| i.duration).is_some_and(|d| d > 12.0), "audio files are measured for the estimate");
    let settings = RunSettings {
        audio_transcript: Some(TranscriptSettings { timestamps: true }),
        video_transcript: Some(TranscriptSettings { timestamps: false }),
        ..Default::default()
    };
    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();
    let targets: Vec<_> = plan.jobs.iter().map(|j| j.target.clone()).collect();
    assert_eq!(targets, [PathBuf::from("termin-kunde.txt"), PathBuf::from("videocall.txt")]);
    let progress = std::sync::Mutex::new(Vec::new());
    let report = execute(
        &plan,
        &|e| {
            if let Event::FileProgress { fraction, .. } = e {
                progress.lock().unwrap().push(fraction);
            }
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!((report.transcripts, report.bytes_in), (2, 0), "transcripts don't count as „gespart“");
    assert!(progress.into_inner().unwrap().len() > 2, "the bar moves while Whisper writes");

    let out = &plan.output_root;
    let audio = std::fs::read_to_string(out.join("termin-kunde.txt")).unwrap();
    assert!(audio.starts_with("Transkript von „Termin Kunde.m4a“\n\n[00:00:0"), "{audio}");
    for word in ["Guten Tag", "Projekt", "Kunden", "Januar"] {
        assert!(audio.contains(word), "„{word}“ missing in:\n{audio}");
    }
    let first = audio.lines().nth(2).unwrap();
    assert!(first >= "[00:00:07]", "the speech starts after the music: {first}");
    assert!(!audio.to_lowercase().contains("musik"), "{audio}");

    let video = std::fs::read_to_string(out.join("videocall.txt")).unwrap();
    assert!(video.contains("Projekt") && !video.contains("[00:"), "no timestamps when not wanted:\n{video}");
}

#[test]
fn cancelling_stops_a_long_transcript() {
    if !ready() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Lang");
    recording(&dir, "Lange Sitzung.m4a", 12);
    let ws = Workspace::open(std::slice::from_ref(&dir)).unwrap();
    let plan = ws.plan(&RunSettings { audio_transcript: Some(TranscriptSettings::default()), ..Default::default() }, RerunPolicy::OnlyNew).unwrap();
    let cancel = AtomicBool::new(false);
    let cancelled_at = std::sync::Mutex::new(None);
    let report = execute(
        &plan,
        &|e| {
            // Cancel once Whisper is properly under way (past decoding the sound).
            if let Event::FileProgress { fraction, .. } = e {
                if fraction > 0.1 && !cancel.swap(true, Ordering::SeqCst) {
                    *cancelled_at.lock().unwrap() = Some(Instant::now());
                }
            }
        },
        &cancel,
    )
    .unwrap();
    let waited = cancelled_at.into_inner().unwrap().expect("progress past 10 %").elapsed();
    assert!(waited < Duration::from_secs(2), "stopped {waited:?} after cancelling");
    assert!(report.cancelled && report.transcripts == 0);
    assert!(!plan.output_root.join("lange-sitzung.txt").exists(), "nothing half-written");
}
