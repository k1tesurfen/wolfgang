//! Videos and audio, converted by Wolfgang's own bundled ffmpeg
//! (built by `scripts/build-ffmpeg.sh`, never the one from Homebrew).

use crate::error::{io_err, Error, Result};
use crate::image::{self, ColorTarget, ImageTask};
use crate::settings::{AudioContent, AudioFormat, AudioSettings, Background, ImageFormat, VideoFormat, VideoQuality, VideoSettings};
use serde::Deserialize;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// Folder holding `ffmpeg` and `ffprobe`, overriding the search (tests, odd setups).
pub const DIR_ENV: &str = "WOLFGANG_FFMPEG_DIR";

/// Frame rates above this are reduced (slow-motion phone videos are 120–240 fps).
const MAX_FPS: f64 = 60.0;
/// Where in the video the poster frame is taken (the first frame is often black).
const POSTER_AT: f64 = 0.1;
const POSTER_QUALITY: f32 = 80.0;

#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// The bundled tools, if they can be found: `$WOLFGANG_FFMPEG_DIR`, next to the
/// running program (symlinks resolved, so `/usr/local/bin/wolfgang` → the app works),
/// `../libexec/wolfgang` (install.sh), or `target/ffmpeg/bin` of the source tree.
pub fn tools() -> Option<&'static Tools> {
    static TOOLS: OnceLock<Option<Tools>> = OnceLock::new();
    TOOLS.get_or_init(find_tools).as_ref()
}

fn find_tools() -> Option<Tools> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os(DIR_ENV) {
        dirs.push(PathBuf::from(d));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()).and_then(|e| e.parent().map(Path::to_path_buf)) {
        dirs.push(exe_dir.join("../libexec/wolfgang"));
        dirs.insert(dirs.len() - 1, exe_dir);
    }
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ffmpeg/bin"));
    dirs.into_iter().find_map(|d| {
        let t = Tools { ffmpeg: d.join("ffmpeg"), ffprobe: d.join("ffprobe") };
        (t.ffmpeg.is_file() && t.ffprobe.is_file()).then_some(t)
    })
}

fn missing_tools() -> Error {
    Error::Media("Mein Werkzeug für Videos und Audio (ffmpeg) fehlt. Installier Wolfgang bitte neu.".into())
}

// ── Looking at a file ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    /// Seconds, if known.
    pub duration: Option<f64>,
    pub video: Option<VideoStream>,
    pub audio: Option<AudioStream>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoStream {
    /// As displayed, i.e. with the rotation already applied.
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    /// HLG or PQ: needs tone mapping to look right as SDR.
    pub hdr: bool,
    /// ffmpeg's names, e.g. `h264` / `yuv420p`.
    pub codec: String,
    pub pix_fmt: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioStream {
    pub channels: u32,
    pub sample_rate: u32,
    pub codec: String,
}

pub fn probe(path: &Path) -> Result<MediaInfo> {
    let tools = tools().ok_or_else(missing_tools)?;
    let out = Command::new(&tools.ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_streams", "-show_format"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(io_err(&tools.ffprobe))?;
    if !out.status.success() {
        return Err(Error::Media(format!("Kann {} nicht lesen: {}", file_name(path), last_line(&out.stderr))));
    }
    parse_probe(&out.stdout).ok_or_else(|| Error::Media(format!("Kann {} nicht lesen", file_name(path))))
}

#[derive(Deserialize)]
struct ProbeJson {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ProbeStream {
    codec_type: String,
    codec_name: String,
    pix_fmt: String,
    width: u32,
    height: u32,
    avg_frame_rate: String,
    color_transfer: String,
    channels: u32,
    sample_rate: String,
    disposition: Disposition,
    side_data_list: Vec<SideData>,
    tags: Tags,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Disposition {
    attached_pic: u8,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SideData {
    rotation: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Tags {
    rotate: Option<String>,
}

fn parse_probe(json: &[u8]) -> Option<MediaInfo> {
    let p: ProbeJson = serde_json::from_slice(json).ok()?;
    let mut info = MediaInfo { duration: p.format.and_then(|f| f.duration?.parse().ok()).filter(|d: &f64| *d > 0.0), ..Default::default() };
    for s in p.streams {
        match s.codec_type.as_str() {
            // Cover art in audio files shows up as a one-frame "video".
            "video" if info.video.is_none() && s.disposition.attached_pic == 0 && s.width > 0 => {
                let rotation = s.side_data_list.iter().map(|d| d.rotation).find(|r| *r != 0.0).or_else(|| s.tags.rotate.and_then(|r| r.parse().ok())).unwrap_or(0.0);
                let quarter = ((rotation / 90.0).round() as i64).rem_euclid(2) == 1;
                let (width, height) = if quarter { (s.height, s.width) } else { (s.width, s.height) };
                info.video = Some(VideoStream {
                    width,
                    height,
                    fps: parse_rate(&s.avg_frame_rate),
                    hdr: matches!(s.color_transfer.as_str(), "arib-std-b67" | "smpte2084"),
                    codec: s.codec_name,
                    pix_fmt: s.pix_fmt,
                });
            }
            "audio" if info.audio.is_none() => {
                info.audio = Some(AudioStream { channels: s.channels, sample_rate: s.sample_rate.parse().unwrap_or(0), codec: s.codec_name });
            }
            _ => {}
        }
    }
    Some(info)
}

/// `30000/1001` → 29.97
fn parse_rate(r: &str) -> Option<f64> {
    let (n, d) = r.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (n > 0.0 && d > 0.0).then(|| n / d)
}

// ── Video ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct VideoTask {
    pub settings: VideoSettings,
    /// Poster frame target relative to the output root, if one is wanted.
    pub poster: Option<PathBuf>,
}

/// Output size: the shorter side becomes `size` (never upscaled), both sides even.
pub fn video_size(width: u32, height: u32, size: Option<u32>) -> (u32, u32) {
    let short = width.min(height).max(1);
    let scale = size.filter(|s| *s < short).map_or(1.0, |s| s as f64 / short as f64);
    let even = |v: u32| ((v as f64 * scale / 2.0).round() as u32 * 2).max(2);
    (even(width), even(height))
}

impl VideoStream {
    /// Frames with transparency (animated GIFs, some ProRes/WebM).
    pub fn has_alpha(&self) -> bool {
        let p = self.pix_fmt.as_str();
        p.starts_with("yuva") || p.starts_with("gbrap") || p.starts_with("rgba") || p.starts_with("bgra") || p.starts_with("argb") || p.starts_with("abgr") || p.starts_with("ya")
    }
}

/// The filter chain from decoded frames to what gets encoded (rotation is
/// applied by ffmpeg before this). `pix_fmt` is the final pixel format.
fn video_filters(v: &VideoStream, size: Option<u32>, background: Background, pix_fmt: &str) -> String {
    let (w, h) = video_size(v.width, v.height, size);
    let mut f = Vec::new();
    if v.has_alpha() {
        // Transparent areas onto a plain background: an opaque filled copy of each
        // frame with the frame on top, so the timing stays the source's. Both
        // branches of `split` share one pixel format, so it has to be RGBA.
        f.push(format!("format=rgba,split[fg][m];[m]drawbox=c={}:t=fill:replace=1[bg];[bg][fg]overlay=format=auto", background.color()));
    }
    if v.hdr {
        // HLG/PQ → SDR with reference white at 203 nits (BT.2408). Without this,
        // iPhone videos come out grey and flat.
        f.push(format!("scale={w}:{h}:flags=lanczos"));
        f.push("zscale=t=linear:npl=203,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=mobius:desat=0,zscale=t=bt709:m=bt709:r=tv".into());
    } else {
        // Everything ends up as BT.709 (what browsers assume); untagged SD video counts as BT.601.
        f.push(format!("scale={w}:{h}:flags=lanczos:out_color_matrix=bt709:out_range=tv"));
    }
    // ffmpeg takes the colour tags for the file from the frames.
    f.push("setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv".into());
    if v.fps.is_some_and(|fps| fps > MAX_FPS + 0.5) {
        f.push(format!("fps={MAX_FPS}"));
    }
    f.push("setsar=1".into());
    f.push(format!("format={pix_fmt}"));
    f.join(",")
}

/// H.264 constant quality (VideoToolbox, 1–100) and VP9 CRF per quality step.
/// Calibrated on 1080p footage: the WebM is about 40 % smaller at a slightly better SSIM.
fn video_quality(format: VideoFormat, q: VideoQuality) -> u32 {
    match (format, q) {
        (VideoFormat::Mp4, VideoQuality::Smaller) => 50,
        (VideoFormat::Mp4, VideoQuality::Balanced) => 58,
        (VideoFormat::Mp4, VideoQuality::Nicer) => 65,
        (VideoFormat::Webm, VideoQuality::Smaller) => 40,
        (VideoFormat::Webm, VideoQuality::Balanced) => 36,
        (VideoFormat::Webm, VideoQuality::Nicer) => 31,
    }
}

/// H.264 quality steps lower for 50/60 fps footage: at q58 it's ~55 % bigger than
/// the same clip at 30 fps, at q54 only ~20 % (SSIM 0.955 instead of 0.964; each
/// frame is on screen for half the time). A bitrate cap would be the obvious fix,
/// but VideoToolbox then *raises* the bitrate of simple clips towards the cap.
const HIGH_FPS_STEP_DOWN: u32 = 4;

/// Bitrate for Apple's software H.264 encoder, which can't do constant quality
/// (Macs where the hardware encoder is busy or missing, e.g. virtual machines).
/// Roughly what the hardware encoder averages on real footage (1080p „Ausgewogen“ ≈ 4.4 Mbit/s).
fn fallback_bitrate(w: u32, h: u32, q: VideoQuality) -> u64 {
    let bits_per_pixel_at_30fps = match q {
        VideoQuality::Smaller => 0.04,
        VideoQuality::Balanced => 0.07,
        VideoQuality::Nicer => 0.11,
    };
    (w as f64 * h as f64 * 30.0 * bits_per_pixel_at_30fps) as u64
}

fn video_args(info: &MediaInfo, s: &VideoSettings, constant_quality: bool) -> Result<Vec<String>> {
    let v = info.video.as_ref().ok_or_else(|| Error::Media("Da ist gar kein Bild drin.".into()))?;
    let mut a: Vec<String> = vec!["-map".into(), "0:v:0".into()];
    let sound = s.sound && info.audio.is_some();
    if sound {
        a.extend(["-map".into(), "0:a:0".into()]);
    }
    a.extend(["-map_metadata", "-1", "-map_chapters", "-1", "-sn", "-dn"].map(String::from));
    a.extend(["-vf".into(), video_filters(v, s.size, s.background, "yuv420p")]);
    match s.format {
        VideoFormat::Mp4 => {
            a.extend(["-c:v", "h264_videotoolbox", "-allow_sw", "1", "-profile:v", "high"].map(String::from));
            if constant_quality {
                let high_fps = v.fps.is_some_and(|f| f > 40.0);
                let q = video_quality(s.format, s.quality) - if high_fps { HIGH_FPS_STEP_DOWN } else { 0 };
                a.extend(["-q:v".into(), q.to_string()]);
            } else {
                let (w, h) = video_size(v.width, v.height, s.size);
                a.extend(["-b:v".into(), fallback_bitrate(w, h, s.quality).to_string()]);
            }
            if sound {
                a.extend(["-c:a", "aac_at", "-b:a", "128k", "-ac", "2"].map(String::from));
            }
            a.extend(["-tag:v", "avc1", "-movflags", "+faststart", "-f", "mp4"].map(String::from));
        }
        VideoFormat::Webm => {
            a.extend(["-c:v".into(), "libvpx-vp9".into(), "-crf".into(), video_quality(s.format, s.quality).to_string()]);
            a.extend(["-b:v", "0", "-row-mt", "1", "-deadline", "good", "-cpu-used", "3"].map(String::from));
            if sound {
                a.extend(["-c:a", "libopus", "-b:a", "96k", "-ac", "2"].map(String::from));
            }
            a.extend(["-f", "webm"].map(String::from));
        }
    }
    Ok(a)
}

/// What became of a video, besides the file itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoResult {
    pub poster: Option<Vec<u8>>,
    /// The converted file was bigger, so the original was taken (only tidied up).
    pub kept_original: bool,
    /// The converted file is bigger than the original, and the original can't be used as it is.
    pub grew: bool,
}

/// Converts one video to `out` (a temporary path; the caller renames it), plus the
/// poster frame if wanted.
pub fn convert_video(src: &Path, task: &VideoTask, out: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<VideoResult> {
    let info = probe(src)?;
    let duration = info.duration;
    // The poster is quick; it counts as the last few percent.
    let video_share = if task.poster.is_some() { 0.97 } else { 1.0 };
    let report = |f: f64| progress(f * video_share);
    let first = ffmpeg(src, &video_args(&info, &task.settings, true)?, out, duration, &report, cancel);
    match first {
        // Constant quality needs the hardware encoder; Apple's software encoder
        // refuses it (and ffmpeg's last words don't always say so). Retry with a bitrate.
        Err(Error::Media(_)) if task.settings.format == VideoFormat::Mp4 => {
            ffmpeg(src, &video_args(&info, &task.settings, false)?, out, duration, &report, cancel)?
        }
        other => other?,
    }
    let mut result = VideoResult::default();
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).map_err(io_err(p));
    let (src_size, out_size) = (size(src)?, size(out)?);
    if out_size > src_size {
        // Already well packed. If the original plays on the web as it is, take it (tidied up).
        if web_ready(&info, &task.settings) {
            let tidy = out.with_extension("tidy");
            match ffmpeg(src, &remux_args(&info, &task.settings), &tidy, duration, &|_| {}, cancel) {
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Ok(()) if size(&tidy)? < out_size => {
                    std::fs::rename(&tidy, out).map_err(io_err(out))?;
                    result.kept_original = true;
                }
                // Didn't work out: the converted file stays.
                _ => {
                    let _ = std::fs::remove_file(&tidy);
                }
            }
        }
        result.grew = !result.kept_original;
    }
    if task.poster.is_some() {
        result.poster = Some(poster(src, &info, &task.settings, cancel)?);
        progress(1.0);
    }
    Ok(result)
}

/// The original already is what the settings ask for: right codecs for the
/// format, no HDR, not bigger than wanted, at most 60 fps.
fn web_ready(info: &MediaInfo, s: &VideoSettings) -> bool {
    let Some(v) = &info.video else { return false };
    let (video_ok, audio_ok): (&[&str], &[&str]) = match s.format {
        VideoFormat::Mp4 => (&["h264"], &["aac"]),
        VideoFormat::Webm => (&["vp9", "vp8"], &["opus", "vorbis"]),
    };
    let audio_fits = !s.sound || info.audio.as_ref().is_none_or(|a| audio_ok.contains(&a.codec.as_str()));
    video_ok.contains(&v.codec.as_str())
        && v.pix_fmt == "yuv420p"
        && !v.hdr
        && v.fps.is_none_or(|f| f <= MAX_FPS + 0.5)
        && s.size.is_none_or(|px| v.width.min(v.height) <= px)
        && audio_fits
}

/// Copies the streams as they are, without metadata, MP4s with quick start.
fn remux_args(info: &MediaInfo, s: &VideoSettings) -> Vec<String> {
    let mut a: Vec<String> = ["-map", "0:v:0"].map(String::from).to_vec();
    if s.sound && info.audio.is_some() {
        a.extend(["-map", "0:a:0"].map(String::from));
    }
    a.extend(["-c", "copy", "-map_metadata", "-1", "-map_chapters", "-1"].map(String::from));
    match s.format {
        VideoFormat::Mp4 => a.extend(["-tag:v", "avc1", "-movflags", "+faststart", "-f", "mp4"].map(String::from)),
        VideoFormat::Webm => a.extend(["-f", "webm"].map(String::from)),
    }
    a
}

/// Pixels per second (M2 Pro, 2026-11): reading an HEVC 4K source, encoding VP9
/// (`-cpu-used 3`), encoding H.264 in hardware. MP4 is limited by reading the
/// source, VP9 by encoding: 30 s of iPhone-like 4K30 → 1080p took 5.5 s as MP4 and 25 s as WebM.
const DECODE_RATE: f64 = 1.3e9;
const VP9_RATE: f64 = 95e6;
const H264_RATE: f64 = 1.5e9;

/// Roughly how long converting these videos takes (`None` if a length is unknown).
pub fn estimate(videos: &[&MediaInfo], s: &VideoSettings) -> Option<Duration> {
    let mut total = 0.0;
    for info in videos {
        let (Some(d), Some(v)) = (info.duration, info.video.as_ref()) else { return None };
        let fps = v.fps.unwrap_or(30.0);
        let (w, h) = video_size(v.width, v.height, s.size);
        let decode = v.width as f64 * v.height as f64 * fps / DECODE_RATE;
        let out = w as f64 * h as f64 * fps.min(MAX_FPS);
        total += d * match s.format {
            VideoFormat::Mp4 => decode.max(out / H264_RATE),
            VideoFormat::Webm => decode + out / VP9_RATE,
        };
    }
    // Several videos run side by side, but VP9 already uses every core.
    let parallel = if videos.len() > 1 { 1.3 } else { 1.0 };
    Some(Duration::from_secs_f64(total / parallel))
}

/// A still from about 10 % into the video, same size as the video, as WebP.
fn poster(src: &Path, info: &MediaInfo, s: &VideoSettings, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let v = info.video.as_ref().expect("checked by video_args");
    let at = info.duration.map_or(0.0, |d| d * POSTER_AT);
    let png = TempFile::new("png");
    let args: Vec<String> = vec!["-frames:v".into(), "1".into(), "-vf".into(), video_filters(v, s.size, s.background, "rgb24"), "-c:v".into(), "png".into(), "-f".into(), "image2".into()];
    ffmpeg_at(src, Some(at), &args, &png.0, None, &|_| {}, cancel)?;
    image::convert(&png.0, &ImageTask { format: ImageFormat::Webp, max_side: None, quality: POSTER_QUALITY, color: ColorTarget::Srgb })
}

// ── Audio ────────────────────────────────────────────────────────────────────

fn audio_args(info: &MediaInfo, s: &AudioSettings) -> Result<Vec<String>> {
    let a_in = info.audio.as_ref().ok_or_else(|| Error::Media("Da ist gar kein Ton drin.".into()))?;
    let mut a: Vec<String> = ["-map", "0:a:0", "-vn", "-sn", "-dn", "-map_metadata", "0"].map(String::from).to_vec();
    let channels = if s.content == AudioContent::Speech { 1 } else { a_in.channels.clamp(1, 2) };
    a.extend(["-ac".into(), channels.to_string()]);
    if a_in.sample_rate > 48_000 {
        a.extend(["-ar", "48000"].map(String::from));
    }
    let rate = |r: &str| ["-b:a".to_owned(), r.to_owned()];
    match s.format {
        AudioFormat::Mp3 => {
            a.extend(["-c:a", "libmp3lame"].map(String::from));
            match s.content {
                AudioContent::Speech => a.extend(rate("64k")),
                AudioContent::Music => a.extend(["-q:a", "2"].map(String::from)), // VBR, ~190 kbit/s
                AudioContent::Best => a.extend(rate("320k")),
            }
            a.extend(["-id3v2_version", "3", "-f", "mp3"].map(String::from));
        }
        AudioFormat::Aac => {
            a.extend(["-c:a", "aac_at"].map(String::from));
            a.extend(rate(match s.content {
                AudioContent::Speech => "64k",
                AudioContent::Music => "192k",
                AudioContent::Best => "256k",
            }));
            a.extend(["-movflags", "+faststart", "-f", "ipod"].map(String::from));
        }
    }
    Ok(a)
}

pub fn convert_audio(src: &Path, s: &AudioSettings, out: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<()> {
    let info = probe(src)?;
    ffmpeg(src, &audio_args(&info, s)?, out, info.duration, progress, cancel)
}

// ── Running ffmpeg ───────────────────────────────────────────────────────────

fn ffmpeg(src: &Path, args: &[String], out: &Path, duration: Option<f64>, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<()> {
    ffmpeg_at(src, None, args, out, duration, progress, cancel)
}

/// Runs ffmpeg, reporting progress (0–1) from its `-progress` output. Cancelling
/// stops it right away; a half-written `out` is removed.
fn ffmpeg_at(src: &Path, seek: Option<f64>, args: &[String], out: &Path, duration: Option<f64>, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<()> {
    let tools = tools().ok_or_else(missing_tools)?;
    let mut cmd = Command::new(&tools.ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-y", "-loglevel", "error", "-nostats", "-progress", "pipe:1"]);
    if let Some(at) = seek {
        cmd.args(["-ss", &format!("{at:.3}")]);
    }
    cmd.arg("-i").arg(src).args(args).arg(out);
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(io_err(&tools.ffmpeg))?;
    let stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");

    let (status, errors) = std::thread::scope(|s| {
        s.spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
                // `out_time_us` (older ffmpeg: the misnamed `out_time_ms`) is in microseconds.
                let us = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms=")).and_then(|v| v.parse::<f64>().ok());
                if let (Some(us), Some(d)) = (us, duration) {
                    progress((us / 1e6 / d).clamp(0.0, 1.0));
                }
            }
        });
        let errors = s.spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf);
            buf
        });
        let status = loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            match child.try_wait() {
                Ok(Some(st)) => break Some(Ok(st)),
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => break Some(Err(e)),
            }
        };
        (status, errors.join().unwrap_or_default())
    });
    match status {
        None => {
            let _ = std::fs::remove_file(out);
            Err(Error::Cancelled)
        }
        Some(Err(e)) => Err(io_err(&tools.ffmpeg)(e)),
        Some(Ok(st)) if st.success() => Ok(()),
        Some(Ok(_)) => {
            let _ = std::fs::remove_file(out);
            Err(Error::Media(last_line(&errors)))
        }
    }
}

fn last_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("ffmpeg ist ohne Meldung gestolpert");
    // `[h264_videotoolbox @ 0x…] Error …` → keep the codec name, drop the address.
    match line.split_once(" @ 0x").and_then(|(head, rest)| Some((head, rest.split_once(']')?.1))) {
        Some((head, rest)) => format!("{head}]{rest}"),
        None => line.to_owned(),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A temporary file in the system temp folder, removed when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("wolfgang-{}-{n}.{ext}", std::process::id())))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_the_shorter_side_and_never_upscale() {
        assert_eq!(video_size(3840, 2160, Some(1080)), (1920, 1080));
        assert_eq!(video_size(1080, 1920, Some(720)), (720, 1280), "vertical video");
        assert_eq!(video_size(1280, 720, Some(1080)), (1280, 720), "no upscaling");
        assert_eq!(video_size(1281, 721, None), (1282, 722), "even sides for H.264");
        assert_eq!(video_size(1440, 1080, Some(720)), (960, 720));
    }

    #[test]
    fn reads_ffprobe_output() {
        let json = br#"{
          "streams": [
            {"codec_type":"video","codec_name":"hevc","pix_fmt":"yuv420p10le","width":1920,"height":1080,"avg_frame_rate":"240/1","color_transfer":"arib-std-b67",
             "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]},
            {"codec_type":"audio","codec_name":"aac","channels":6,"sample_rate":"48000"},
            {"codec_type":"data"}
          ],
          "format": {"duration":"12.500000"}
        }"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(info.duration, Some(12.5));
        assert_eq!(info.video, Some(VideoStream { width: 1080, height: 1920, fps: Some(240.0), hdr: true, codec: "hevc".into(), pix_fmt: "yuv420p10le".into() }));
        assert_eq!(info.audio, Some(AudioStream { channels: 6, sample_rate: 48000, codec: "aac".into() }));

        let cover = br#"{"streams":[{"codec_type":"audio","channels":2,"sample_rate":"44100"},
            {"codec_type":"video","width":600,"height":600,"disposition":{"attached_pic":1}}],"format":{}}"#;
        let info = parse_probe(cover).unwrap();
        assert!(info.video.is_none(), "cover art is not a video");
        assert_eq!(info.duration, None);
    }

    #[test]
    fn video_filters_fix_hdr_and_slow_motion() {
        let v = VideoStream { width: 1920, height: 1080, fps: Some(30.0), ..Default::default() };
        assert_eq!(
            video_filters(&v, Some(720), Background::White, "yuv420p"),
            "scale=1280:720:flags=lanczos:out_color_matrix=bt709:out_range=tv,setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709:range=tv,setsar=1,format=yuv420p"
        );
        let hdr = VideoStream { hdr: true, fps: Some(240.0), ..v.clone() };
        let f = video_filters(&hdr, None, Background::White, "yuv420p");
        assert!(f.contains("tonemap=tonemap=mobius") && f.contains("fps=60"), "{f}");
        let gif = VideoStream { pix_fmt: "bgra".into(), ..v.clone() };
        assert!(video_filters(&gif, None, Background::Black, "yuv420p").starts_with("format=rgba,split[fg][m];[m]drawbox=c=black:t=fill:replace=1"));
    }

    #[test]
    fn video_arguments() {
        let info = MediaInfo {
            duration: Some(3.0),
            video: Some(VideoStream { width: 1920, height: 1080, fps: Some(30.0), ..Default::default() }),
            audio: Some(AudioStream { channels: 2, sample_rate: 48000, ..Default::default() }),
        };
        let mp4 = video_args(&info, &VideoSettings::default(), true).unwrap().join(" ");
        assert!(mp4.contains("h264_videotoolbox") && mp4.contains("-q:v 58") && mp4.contains("aac_at") && mp4.contains("+faststart"), "{mp4}");
        assert!(mp4.contains("-map_metadata -1"), "GPS and other metadata are dropped");
        let fallback = video_args(&info, &VideoSettings::default(), false).unwrap().join(" ");
        assert!(fallback.contains("-b:v 4354560") && !fallback.contains("-q:v"), "{fallback}");
        let fast = MediaInfo { video: Some(VideoStream { fps: Some(59.94), ..info.video.clone().unwrap() }), ..info.clone() };
        assert!(video_args(&fast, &VideoSettings::default(), true).unwrap().join(" ").contains("-q:v 54"), "60 fps steps down");

        let silent = VideoSettings { format: VideoFormat::Webm, sound: false, ..Default::default() };
        let webm = video_args(&info, &silent, true).unwrap().join(" ");
        assert!(webm.contains("libvpx-vp9 -crf 36") && !webm.contains("0:a:0") && !webm.contains("opus"), "{webm}");

        let no_audio = MediaInfo { audio: None, ..info.clone() };
        assert!(!video_args(&no_audio, &VideoSettings::default(), true).unwrap().join(" ").contains("aac_at"));
        assert!(video_args(&MediaInfo::default(), &VideoSettings::default(), true).is_err());
    }

    #[test]
    fn audio_arguments() {
        let info = MediaInfo { audio: Some(AudioStream { channels: 6, sample_rate: 96000, ..Default::default() }), ..Default::default() };
        let speech = audio_args(&info, &AudioSettings { format: AudioFormat::Mp3, content: AudioContent::Speech }).unwrap().join(" ");
        assert!(speech.contains("-ac 1") && speech.contains("-b:a 64k") && speech.contains("-ar 48000"), "{speech}");
        let music = audio_args(&info, &AudioSettings { format: AudioFormat::Aac, content: AudioContent::Music }).unwrap().join(" ");
        assert!(music.contains("-ac 2") && music.contains("aac_at -b:a 192k") && music.contains("-f ipod"), "{music}");
        assert!(audio_args(&MediaInfo::default(), &AudioSettings::default()).is_err());
    }

    #[test]
    fn web_ready_originals_and_estimates() {
        let h264 = MediaInfo {
            duration: Some(30.0),
            video: Some(VideoStream { width: 1920, height: 1080, fps: Some(30.0), codec: "h264".into(), pix_fmt: "yuv420p".into(), hdr: false }),
            audio: Some(AudioStream { channels: 2, sample_rate: 48000, codec: "aac".into() }),
        };
        let mp4 = VideoSettings::default();
        assert!(web_ready(&h264, &mp4));
        assert!(!web_ready(&h264, &VideoSettings { size: Some(720), ..mp4.clone() }), "too big for 720p");
        assert!(!web_ready(&h264, &VideoSettings { format: VideoFormat::Webm, ..mp4.clone() }));
        let mut hevc = h264.clone();
        hevc.video.as_mut().unwrap().codec = "hevc".into();
        assert!(!web_ready(&hevc, &mp4));
        let mut mp3 = h264.clone();
        mp3.audio.as_mut().unwrap().codec = "mp3".into();
        assert!(!web_ready(&mp3, &mp4) && web_ready(&mp3, &VideoSettings { sound: false, ..mp4.clone() }), "sound is dropped anyway");
        assert!(remux_args(&h264, &mp4).join(" ").contains("-c copy -map_metadata -1"));

        // The measured 4K30 clip: ~5.5 s as MP4, ~25 s as WebM.
        let iphone = MediaInfo { video: Some(VideoStream { width: 3840, height: 2160, ..h264.video.clone().unwrap() }), ..h264.clone() };
        let secs = |s: &VideoSettings| estimate(&[&iphone], s).unwrap().as_secs_f64();
        assert!((4.0..8.0).contains(&secs(&mp4)), "{}", secs(&mp4));
        assert!((20.0..30.0).contains(&secs(&VideoSettings { format: VideoFormat::Webm, ..mp4.clone() })));
        assert_eq!(estimate(&[&MediaInfo::default()], &mp4), None);
    }

    #[test]
    fn ffmpeg_errors_are_shortened() {
        assert_eq!(last_line(b"x\n[h264_videotoolbox @ 0x8e5035180] Error setting bitrate\n\n"), "[h264_videotoolbox] Error setting bitrate");
        assert_eq!(last_line(b"Invalid data found\n"), "Invalid data found");
    }
}
