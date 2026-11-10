//! „Nur aufschreiben, was gesagt wird“: German transcripts of meeting recordings,
//! made on this Mac by whisper.cpp (Metal). The model isn't part of the app; it's an
//! add-on (≈ 575 MB) installed once in the settings, see [`install`].
//!
//! How (decided by the spike, PLAN.md milestone 11): the bundled ffmpeg decodes the
//! sound to 16 kHz mono; a small voice detector (Silero VAD) finds where people talk;
//! only those parts, joined, go to Whisper (`large-v3-turbo`, German fixed), and its
//! timestamps are mapped back to the recording. Without the joining, waiting music
//! became „... Musik ...“ and swallowed what was said next to it; silencing the music
//! instead made Whisper invent text in the silence.

use crate::error::{io_err, Error, Result};
use crate::media::{self, TempFile};
use crate::settings::TranscriptSettings;
use crate::store::UserStore;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Folder holding the model files, overriding the default (tests, development).
pub const DIR_ENV: &str = "WOLFGANG_MODELS_DIR";

/// One file of the add-on, pinned to a commit so it can't change under us.
#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub blake3: &'static str,
}

/// Whisper `large-v3-turbo`, quantized (MIT, OpenAI; converted by whisper.cpp).
pub const WHISPER: ModelFile = ModelFile {
    name: "ggml-large-v3-turbo-q5_0.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
    size: 574_041_195,
    blake3: "75830bd5fd871c708d620f37df07ea9ead205dadb4cd2f24bad7670976f92e98",
};

/// Silero voice detector (MIT), finds where people talk.
pub const VAD: ModelFile = ModelFile {
    name: "ggml-silero-v5.1.2.bin",
    url: "https://huggingface.co/ggml-org/whisper-vad/resolve/9ffd54a1e1ee413ddf265af9913beaf518d1639b/ggml-silero-v5.1.2.bin",
    size: 885_098,
    blake3: "6bae4756cb1f3ef603aeb2b294c764aa36b7f2aeb809eeeec9569fcec22a13a4",
};

pub const FILES: [ModelFile; 2] = [WHISPER, VAD];

const MISSING: &str = "Fürs Aufschreiben fehlt mir noch das Zusatzmodul. Das installierst du einmal in den Einstellungen.";
const CURL: &str = "/usr/bin/curl";

/// How much faster than realtime Whisper writes (M2 Pro, 2026-11: 40 minutes in 2 min 15 s).
const SPEED: f64 = 16.0;
/// Decoding the sound is quick; it counts as the first few percent.
const DECODE_SHARE: f64 = 0.03;
const SAMPLE_RATE: usize = 16_000;
/// Speech parts are padded a little, so no word gets cut off.
const PAD_SECONDS: f64 = 0.2;
/// A pause this long (seconds) starts a new paragraph …
const PARAGRAPH_PAUSE: f64 = 1.0;
/// … and so does the end of a sentence once a paragraph has this many words.
const PARAGRAPH_WORDS: usize = 60;

/// Size of the whole add-on in bytes.
pub fn install_size() -> u64 {
    FILES.iter().map(|f| f.size).sum()
}

/// `$WOLFGANG_MODELS_DIR`, else `models/` next to the user's settings
/// (`~/Library/Application Support/Wolfgang/models`).
pub fn models_dir() -> PathBuf {
    if let Some(d) = std::env::var_os(DIR_ENV) {
        return PathBuf::from(d);
    }
    UserStore::default_path().parent().map_or_else(|| PathBuf::from("models"), |d| d.join("models"))
}

/// The add-on is there (checked by size; the content was checked when it was installed).
pub fn installed() -> bool {
    installed_in(&models_dir())
}

fn installed_in(dir: &Path) -> bool {
    FILES.iter().all(|f| std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.size))
}

// ── Installing ───────────────────────────────────────────────────────────────

/// Downloads the add-on into [`models_dir`] with the Mac's own `curl`, reporting
/// (bytes so far, total). Every file is checked against its pinned hash; a cancelled
/// download continues where it stopped next time.
pub fn install(progress: &(dyn Fn(u64, u64) + Sync), cancel: &AtomicBool) -> Result<()> {
    install_into(&models_dir(), &FILES, progress, cancel)
}

fn install_into(dir: &Path, files: &[ModelFile], progress: &(dyn Fn(u64, u64) + Sync), cancel: &AtomicBool) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    let total = files.iter().map(|f| f.size).sum();
    let mut before = 0;
    for f in files.iter().copied() {
        let target = dir.join(f.name);
        if !std::fs::metadata(&target).is_ok_and(|m| m.len() == f.size) {
            let part = dir.join(format!(".{}.part", f.name));
            download(&f, &part, &|n| progress(before + n.min(f.size), total), cancel)?;
            if hash(&part)? != f.blake3 {
                let _ = std::fs::remove_file(&part);
                return Err(Error::Media(format!("„{}“ kam kaputt an. Versuch's bitte nochmal.", f.name)));
            }
            std::fs::rename(&part, &target).map_err(io_err(&target))?;
        }
        before += f.size;
        progress(before, total);
    }
    Ok(())
}

fn download(f: &ModelFile, part: &Path, progress: &dyn Fn(u64), cancel: &AtomicBool) -> Result<()> {
    let size = || std::fs::metadata(part).map_or(0, |m| m.len());
    if size() > f.size {
        let _ = std::fs::remove_file(part); // can't be resumed
    }
    if size() == f.size {
        return Ok(()); // complete, just not checked yet
    }
    let mut child = Command::new(CURL)
        .args(["--fail", "--location", "--silent", "--show-error", "--retry", "3", "--continue-at", "-", "--output"])
        .arg(part)
        .arg(f.url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io_err(Path::new(CURL)))?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                progress(size());
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(io_err(Path::new(CURL))(e)),
        }
    };
    if !status.success() || size() != f.size {
        let mut msg = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut msg);
        }
        let why = msg.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("ohne Meldung").to_owned();
        return Err(Error::Media(format!("Das Herunterladen hat nicht geklappt ({why}). Bist du online?")));
    }
    Ok(())
}

fn hash(path: &Path) -> Result<String> {
    let mut h = blake3::Hasher::new();
    h.update_reader(std::fs::File::open(path).map_err(io_err(path))?).map_err(io_err(path))?;
    Ok(h.finalize().to_hex().to_string())
}

/// Removes the add-on (and half-finished downloads).
pub fn remove() -> Result<()> {
    let dir = models_dir();
    for f in FILES {
        for p in [dir.join(f.name), dir.join(format!(".{}.part", f.name))] {
            match std::fs::remove_file(&p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io_err(&p)(e)),
                _ => {}
            }
        }
    }
    let _ = std::fs::remove_dir(&dir); // only if nothing else is in there
    Ok(())
}

// ── Writing down ─────────────────────────────────────────────────────────────

/// Roughly how long writing down recordings of these lengths (seconds) takes.
/// They're done one after another.
pub fn estimate(durations: &[f64]) -> Duration {
    Duration::from_secs_f64(durations.iter().sum::<f64>() / SPEED)
}

/// Whisper takes the GPU and about 1.2 GB of memory: one recording at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Writes down what's said in `src` (any video or audio file ffmpeg reads), as the
/// text of the `.txt` file.
pub fn transcribe(src: &Path, s: &TranscriptSettings, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<String> {
    let dir = models_dir();
    if !installed_in(&dir) {
        return Err(Error::Media(MISSING.into()));
    }
    let info = media::probe(src)?;
    let wav = TempFile::new("wav");
    media::speech_wav(src, &info, &wav.0, &|f| progress(f * DECODE_SHARE), cancel)?;
    let audio = read_wav(&wav.0)?;
    drop(wav);

    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let segments = engine::run(&dir, &audio, &|f| progress(DECODE_SHARE + f * (1.0 - DECODE_SHARE)), cancel)?;
    let paragraphs = paragraphs(&segments);
    if paragraphs.is_empty() {
        return Err(Error::Media("Da hab ich niemanden reden hören.".into()));
    }
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(render(&name, &paragraphs, s.timestamps))
}

/// 16-bit PCM WAV (as written by [`media::speech_wav`]) → samples −1…1.
fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let b = std::fs::read(path).map_err(io_err(path))?;
    let broken = || Error::Media("Den Ton konnte ich nicht lesen.".into());
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(broken());
    }
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().expect("4 bytes")) as usize;
        if &b[i..i + 4] == b"data" {
            let data = &b[i + 8..(i + 8).saturating_add(len).min(b.len())];
            return Ok(data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect());
        }
        i += 8 + len + (len & 1);
    }
    Err(broken())
}

/// One piece Whisper wrote, in seconds of the recording.
#[derive(Debug, Clone, PartialEq)]
struct Segment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Paragraph {
    start: f64,
    text: String,
}

/// Speech parts from the voice detector (centiseconds) → padded sample ranges,
/// overlapping ones merged.
fn speech_parts(found: &[(f32, f32)], len: usize) -> Vec<(usize, usize)> {
    let pad = (PAD_SECONDS * SAMPLE_RATE as f64) as usize;
    let at = |cs: f32| (cs as f64 / 100.0 * SAMPLE_RATE as f64) as usize;
    let mut parts: Vec<(usize, usize)> = Vec::new();
    for &(start, end) in found {
        let (a, b) = (at(start).saturating_sub(pad).min(len), (at(end) + pad).min(len));
        match parts.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ if a < b => parts.push((a, b)),
            _ => {}
        }
    }
    parts
}

/// The parts back to back, and where each starts (in the joined audio, in the recording).
fn join_parts(audio: &[f32], parts: &[(usize, usize)]) -> (Vec<f32>, Vec<(usize, usize)>) {
    let mut joined = Vec::with_capacity(parts.iter().map(|(a, b)| b - a).sum());
    let mut map = Vec::with_capacity(parts.len());
    for &(a, b) in parts {
        map.push((joined.len(), a));
        joined.extend_from_slice(&audio[a..b]);
    }
    (joined, map)
}

/// A Whisper timestamp (centiseconds of the joined audio) → seconds of the recording.
fn to_recording(map: &[(usize, usize)], cs: i64) -> f64 {
    let x = cs.max(0) as usize * SAMPLE_RATE / 100;
    let (joined, orig) = map.iter().rev().find(|(j, _)| *j <= x).copied().unwrap_or((0, 0));
    (orig + x - joined) as f64 / SAMPLE_RATE as f64
}

/// Things Whisper writes when nobody talks: „... Musik ...“, „(Applaus)“, and
/// credits it learned from German TV subtitles.
fn is_noise(text: &str) -> bool {
    let t = text.to_lowercase();
    let bare: String = t.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    matches!(bare.trim(), "" | "musik" | "applaus" | "lachen" | "stille")
        || (t.contains("untertitel") && (t.contains("zdf") || t.contains("amara")))
}

fn paragraphs(segments: &[Segment]) -> Vec<Paragraph> {
    let mut out: Vec<Paragraph> = Vec::new();
    let (mut words, mut last_end) = (0, f64::NEG_INFINITY);
    for s in segments.iter().filter(|s| !is_noise(&s.text)) {
        let text = s.text.trim();
        let long = words >= PARAGRAPH_WORDS && out.last().is_some_and(|p| p.text.ends_with(['.', '?', '!']));
        match out.last_mut() {
            Some(p) if s.start - last_end < PARAGRAPH_PAUSE && !long => {
                p.text.push(' ');
                p.text.push_str(text);
            }
            _ => {
                out.push(Paragraph { start: s.start, text: text.to_owned() });
                words = 0;
            }
        }
        words += text.split_whitespace().count();
        last_end = s.end;
    }
    out
}

/// `[00:12:30]`
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("[{:02}:{:02}:{:02}]", s / 3600, s / 60 % 60, s % 60)
}

fn render(name: &str, paragraphs: &[Paragraph], timestamps: bool) -> String {
    let mut out = format!("Transkript von „{name}“\n");
    for p in paragraphs {
        out.push('\n');
        if timestamps {
            out.push_str(&clock(p.start));
            out.push(' ');
        }
        out.push_str(&p.text);
        out.push('\n');
    }
    out
}

#[cfg(target_os = "macos")]
mod engine {
    use super::*;
    use std::sync::atomic::AtomicI32;
    use std::sync::Arc;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

    fn failed(e: impl std::fmt::Display) -> Error {
        Error::Media(format!("Beim Aufschreiben ist was schiefgegangen: {e}"))
    }

    /// Voice detector → joined speech → Whisper (on its own thread, so cancelling
    /// and progress work while it runs) → segments in seconds of the recording.
    pub(super) fn run(dir: &Path, audio: &[f32], progress: &dyn Fn(f64), cancel: &AtomicBool) -> Result<Vec<Segment>> {
        whisper_rs::install_logging_hooks(); // whisper.cpp would talk on stderr otherwise
        let vad_model = dir.join(VAD.name);
        let mut vad = WhisperVadContext::new(&vad_model.to_string_lossy(), WhisperVadContextParams::new()).map_err(failed)?;
        let found = vad.segments_from_samples(WhisperVadParams::new(), audio).map_err(failed)?;
        let found: Vec<(f32, f32)> = (0..found.num_segments()).filter_map(|i| found.get_segment(i)).map(|s| (s.start, s.end)).collect();
        drop(vad);
        let (joined, map) = join_parts(audio, &speech_parts(&found, audio.len()));
        if joined.is_empty() {
            return Ok(Vec::new());
        }

        let percent = Arc::new(AtomicI32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let raw = std::thread::scope(|scope| {
            let worker = scope.spawn(|| whisper(&dir.join(WHISPER.name), &joined, percent.clone(), stop.clone()));
            while !worker.is_finished() {
                if cancel.load(Ordering::Relaxed) {
                    stop.store(true, Ordering::Relaxed);
                }
                progress(percent.load(Ordering::Relaxed).clamp(0, 100) as f64 / 100.0);
                std::thread::sleep(Duration::from_millis(100));
            }
            worker.join().unwrap_or_else(|_| Err(failed("Whisper ist abgestürzt")))
        })?;
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        Ok(raw.into_iter().map(|(a, b, text)| Segment { start: to_recording(&map, a), end: to_recording(&map, b), text }).collect())
    }

    /// whisper-rs 0.16's `set_abort_callback_safe` casts its boxed closure back to the
    /// wrong type (whisper.cpp then reads garbage as „stop“ and fails with -6), so
    /// cancelling uses a plain C callback on the stop flag.
    unsafe extern "C" fn stop_requested(flag: *mut std::ffi::c_void) -> bool {
        // SAFETY: always the `AtomicBool` set as user data in `whisper`.
        unsafe { (*(flag as *const AtomicBool)).load(Ordering::Relaxed) }
    }

    fn whisper(model: &Path, audio: &[f32], percent: Arc<AtomicI32>, stop: Arc<AtomicBool>) -> Result<Vec<(i64, i64, String)>> {
        let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default()).map_err(failed)?;
        let mut state = ctx.create_state().map_err(failed)?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // German fixed: detecting decides from the first 30 seconds and can pick the
        // wrong language. English words in between come out English anyway.
        p.set_language(Some("de"));
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_progress_callback_safe(move |pct: i32| percent.store(pct, Ordering::Relaxed));
        // SAFETY: `stop` outlives `state.full`, and `stop_requested` only reads the flag.
        unsafe {
            p.set_abort_callback(Some(stop_requested));
            p.set_abort_callback_user_data(Arc::as_ptr(&stop) as *mut std::ffi::c_void);
        }
        let result = state.full(p, audio);
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        result.map_err(failed)?;
        state
            .as_iter()
            .map(|seg| Ok((seg.start_timestamp(), seg.end_timestamp(), seg.to_str_lossy().map_err(failed)?.trim().to_owned())))
            .collect()
    }
}

#[cfg(not(target_os = "macos"))]
mod engine {
    use super::*;

    pub(super) fn run(_: &Path, _: &[f32], _: &dyn Fn(f64), _: &AtomicBool) -> Result<Vec<Segment>> {
        Err(Error::Media("Aufschreiben geht nur auf dem Mac.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, text: &str) -> Segment {
        Segment { start, end, text: text.into() }
    }

    #[test]
    fn speech_parts_are_padded_merged_and_mapped_back() {
        // 10 s of audio, speech at 1–2 s, 2.3–3 s (merged with the first) and 8–9 s.
        let parts = speech_parts(&[(100.0, 200.0), (230.0, 300.0), (800.0, 900.0)], 10 * SAMPLE_RATE);
        let s = |sec: f64| (sec * SAMPLE_RATE as f64).round() as usize;
        assert_eq!(parts, vec![(s(0.8), s(3.2)), (s(7.8), s(9.2))]);
        assert_eq!(speech_parts(&[(990.0, 1200.0)], 10 * SAMPLE_RATE), vec![(s(9.7), s(10.0))], "clipped at the end");

        let audio: Vec<f32> = (0..10 * SAMPLE_RATE).map(|i| i as f32).collect();
        let (joined, map) = join_parts(&audio, &parts);
        assert_eq!(joined.len(), s(2.4) + s(1.4));
        assert_eq!(joined[s(2.4)], audio[s(7.8)], "second part follows the first");
        assert!((to_recording(&map, 0) - 0.8).abs() < 1e-9);
        assert!((to_recording(&map, 250) - 7.9).abs() < 1e-9, "2.5 s into the joined audio = 0.1 s into the second part");
    }

    #[test]
    fn noise_is_dropped() {
        for t in ["... Musik ...", "(Musik)", "*Applaus*", "[MUSIK]", "Untertitel im Auftrag des ZDF, 2021", "Untertitel der Amara.org-Community"] {
            assert!(is_noise(t), "{t}");
        }
        for t in ["Musik ist wichtig.", "Vielen Dank.", "Ja."] {
            assert!(!is_noise(t), "{t}");
        }
    }

    #[test]
    fn paragraphs_at_pauses_and_after_long_stretches() {
        let long = "wort ".repeat(PARAGRAPH_WORDS).trim().to_owned() + ".";
        let p = paragraphs(&[
            seg(41.0, 43.0, "Hallo, guten Tag."),
            seg(43.2, 44.0, "Hallo."),
            seg(45.5, 47.0, "... Musik ..."),
            seg(47.0, 60.0, "Schauen wir mal,"),
            seg(60.1, 70.0, &long),
            seg(70.1, 72.0, "Neuer Absatz."),
        ]);
        let starts: Vec<f64> = p.iter().map(|p| p.start).collect();
        assert_eq!(starts, vec![41.0, 47.0, 70.1], "{p:?}");
        assert_eq!(p[0].text, "Hallo, guten Tag. Hallo.");
        assert!(p[1].text.starts_with("Schauen wir mal, wort"));
    }

    #[test]
    fn renders_with_and_without_timestamps() {
        let p = vec![Paragraph { start: 41.7, text: "Hallo.".into() }, Paragraph { start: 3725.0, text: "Tschüss.".into() }];
        assert_eq!(render("Meeting.m4a", &p, true), "Transkript von „Meeting.m4a“\n\n[00:00:41] Hallo.\n\n[01:02:05] Tschüss.\n");
        assert_eq!(render("Meeting.m4a", &p, false), "Transkript von „Meeting.m4a“\n\nHallo.\n\nTschüss.\n");
    }

    #[test]
    fn installs_checks_resumes_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("model.bin");
        std::fs::write(&src, b"Ein sehr kleines Modell").unwrap();
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let good = ModelFile { name: "m.bin", url: leak(format!("file://{}", src.display())), size: 23, blake3: leak(hash(&src).unwrap()) };
        let models = dir.path().join("models");
        let seen = Mutex::new(Vec::new());
        install_into(&models, &[good], &|done, total| seen.lock().unwrap().push((done, total)), &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(models.join("m.bin")).unwrap(), b"Ein sehr kleines Modell");
        assert_eq!(seen.lock().unwrap().last(), Some(&(23, 23)));

        // Half a download continues where it stopped.
        std::fs::remove_file(models.join("m.bin")).unwrap();
        std::fs::write(models.join(".m.bin.part"), b"Ein sehr").unwrap();
        install_into(&models, &[good], &|_, _| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(models.join("m.bin")).unwrap(), b"Ein sehr kleines Modell");

        let wrong = ModelFile { name: "w.bin", blake3: "00", ..good };
        let e = install_into(&models, &[wrong], &|_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(e.to_string().contains("kaputt") && !models.join(".w.bin.part").exists(), "{e}");
        let gone = ModelFile { name: "g.bin", url: "file:///gibts/nicht", ..good };
        assert!(install_into(&models, &[gone], &|_, _| {}, &AtomicBool::new(false)).unwrap_err().to_string().contains("Herunterladen"));
        assert!(matches!(install_into(&models, &[ModelFile { name: "c.bin", ..good }], &|_, _| {}, &AtomicBool::new(true)), Err(Error::Cancelled)));
    }

    #[test]
    fn reads_wav_and_estimates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let mut b = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
        b.extend([0u8; 16]);
        b.extend(b"LIST\x03\0\0\0abc\0"); // odd chunk, padded
        b.extend(b"data\x04\0\0\0");
        b.extend(i16::MAX.to_le_bytes());
        b.extend(i16::MIN.to_le_bytes());
        std::fs::write(&path, &b).unwrap();
        let s = read_wav(&path).unwrap();
        assert!((s[0] - 1.0).abs() < 1e-4 && s[1] == -1.0, "{s:?}");
        std::fs::write(&path, b"nope").unwrap();
        assert!(read_wav(&path).is_err());
        assert_eq!(estimate(&[2400.0, 1440.0]).as_secs(), 240, "an hour of meetings in four minutes");
    }
}
