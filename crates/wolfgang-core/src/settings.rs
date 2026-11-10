//! What a run should do. Produced by the conversation (or CLI flags),
//! consumed by [`crate::Workspace::plan`], stored in run logs and presets.

use crate::error::{Error, Result};
use crate::naming::Separator;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SIZE_SMALL: u32 = 800;
pub const SIZE_NORMAL: u32 = 1600;
pub const SIZE_LARGE: u32 = 2400;

pub const QUALITY_SMALLER: u8 = 65;
pub const QUALITY_BALANCED: u8 = 80;
pub const QUALITY_NICER: u8 = 90;

pub const MAX_SIDE_RANGE: std::ops::RangeInclusive<u32> = 16..=16384;
pub const QUALITY_RANGE: std::ops::RangeInclusive<u8> = 1..=100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Jpg,
    Webp,
    Avif,
    Png,
}

impl ImageFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Png => "png",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpg => "JPG",
            Self::Webp => "WebP",
            Self::Avif => "AVIF",
            Self::Png => "PNG",
        }
    }
}

/// Web mode only allows modern formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebFormat {
    #[default]
    Webp,
    Avif,
}

impl From<WebFormat> for ImageFormat {
    fn from(f: WebFormat) -> Self {
        match f {
            WebFormat::Webp => ImageFormat::Webp,
            WebFormat::Avif => ImageFormat::Avif,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ImageSettings {
    /// „Hochwertig behalten“: original size, near-lossless, original file names.
    HighQuality { format: ImageFormat },
    /// „Fürs Web vorbereiten“
    Web(WebSettings),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSettings {
    pub format: WebFormat,
    /// Longest side in pixels. Never upscales.
    pub max_side: u32,
    /// 1–100, JPEG-like scale; mapped per codec.
    pub quality: u8,
    pub naming: Naming,
}

impl Default for WebSettings {
    fn default() -> Self {
        Self { format: WebFormat::default(), max_side: SIZE_NORMAL, quality: QUALITY_BALANCED, naming: Naming::default() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Naming {
    pub prefix: String,
    /// Use `*.txt` keyword files found next to images.
    pub use_keyword_files: bool,
    /// Typed-in keywords per folder (relative path, `""` = top level).
    /// Used for folders without a keyword file; they win over `keywords_all`.
    pub folder_keywords: BTreeMap<String, String>,
    /// Typed-in keywords for every folder without a keyword file.
    pub keywords_all: String,
    pub separator: Separator,
    pub append_original: bool,
}

/// Shorter side of „Klein“ / „Normal“ videos (720p / 1080p).
pub const VIDEO_SMALL: u32 = 720;
pub const VIDEO_NORMAL: u32 = 1080;
pub const VIDEO_SIZE_RANGE: std::ops::RangeInclusive<u32> = 144..=4320;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoFormat {
    /// H.264 + AAC: plays everywhere.
    #[default]
    Mp4,
    /// VP9 + Opus: smaller, slower to make.
    Webm,
}

impl VideoFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Webm => "WebM",
        }
    }
}

/// „Kleiner / Ausgewogen / Schöner“, mapped per codec (no free number for videos).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoQuality {
    Smaller,
    #[default]
    Balanced,
    Nicer,
}

impl VideoQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Smaller => "kleiner",
            Self::Balanced => "ausgewogen",
            Self::Nicer => "schöner",
        }
    }
}

/// What goes behind transparent areas of animated GIFs (MP4 can't be transparent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    #[default]
    White,
    Black,
}

impl Background {
    /// ffmpeg's colour name.
    pub fn color(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Black => "black",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::White => "Weiß",
            Self::Black => "Schwarz",
        }
    }
}

/// Videos are always prepared for the web.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoSettings {
    pub format: VideoFormat,
    /// Shorter side in pixels (720 = „720p“, also for vertical videos);
    /// `None` keeps the original size. Never upscales.
    pub size: Option<u32>,
    pub quality: VideoQuality,
    /// Keep the sound track.
    pub sound: bool,
    /// Also write a still image (WebP) named like the video.
    pub poster: bool,
    /// Behind transparent areas of animated GIFs.
    pub background: Background,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self { format: VideoFormat::Mp4, size: Some(VIDEO_NORMAL), quality: VideoQuality::Balanced, sound: true, poster: true, background: Background::White }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    #[default]
    Mp3,
    /// AAC in an `.m4a`.
    Aac,
}

impl AudioFormat {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Aac => "m4a",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::Aac => "AAC",
        }
    }
}

/// „Was ist drauf?“ – Wolfgang picks channels and bitrate from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioContent {
    /// Podcast, interview: mono, small.
    Speech,
    #[default]
    Music,
    /// Music, best quality.
    Best,
}

impl AudioContent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Speech => "Sprache",
            Self::Music => "Musik",
            Self::Best => "Musik in bester Qualität",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub format: AudioFormat,
    pub content: AudioContent,
}

/// „Nur aufschreiben, was gesagt wird“: a German text of a meeting recording,
/// instead of converting the video or audio file (see [`crate::transcribe`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptSettings {
    /// `[00:12:30]` at the start of each paragraph, to find the spot in the recording.
    pub timestamps: bool,
}

impl Default for TranscriptSettings {
    fn default() -> Self {
        Self { timestamps: true }
    }
}

/// Pictures are pulled out of PDFs as they are; nothing to configure.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PdfSettings {}

/// Fonts are converted 1:1 to WOFF2; nothing to configure yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FontSettings {}

/// Settings for one run. A pile with `None` is left alone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunSettings {
    pub images: Option<ImageSettings>,
    pub videos: Option<VideoSettings>,
    pub audio: Option<AudioSettings>,
    /// Videos are written down instead of converted (never together with `videos`).
    pub video_transcript: Option<TranscriptSettings>,
    /// Audio files are written down instead of converted (never together with `audio`).
    pub audio_transcript: Option<TranscriptSettings>,
    pub pdfs: Option<PdfSettings>,
    pub fonts: Option<FontSettings>,
}

impl RunSettings {
    pub fn validate(&self) -> Result<()> {
        if let Some(ImageSettings::Web(w)) = &self.images {
            if !MAX_SIDE_RANGE.contains(&w.max_side) {
                return Err(Error::Settings(format!(
                    "Größe {} px liegt nicht zwischen {} und {}",
                    w.max_side,
                    MAX_SIDE_RANGE.start(),
                    MAX_SIDE_RANGE.end()
                )));
            }
            if !QUALITY_RANGE.contains(&w.quality) {
                return Err(Error::Settings(format!("Qualität {} liegt nicht zwischen 1 und 100", w.quality)));
            }
        }
        if (self.videos.is_some() && self.video_transcript.is_some()) || (self.audio.is_some() && self.audio_transcript.is_some()) {
            return Err(Error::Settings("Aufschreiben oder umwandeln – beides zugleich geht nicht".into()));
        }
        if let Some(size) = self.videos.as_ref().and_then(|v| v.size) {
            if !VIDEO_SIZE_RANGE.contains(&size) {
                return Err(Error::Settings(format!(
                    "Videogröße {size}p liegt nicht zwischen {}p und {}p",
                    VIDEO_SIZE_RANGE.start(),
                    VIDEO_SIZE_RANGE.end()
                )));
            }
        }
        Ok(())
    }
}

/// Maps the user-facing quality (1–100, JPEG-like) to each encoder's own scale,
/// so the same answer looks roughly the same in every format.
/// AVIF reaches the same visual quality at lower numbers.
pub fn encoder_quality(format: ImageFormat, quality: u8) -> f32 {
    let q = quality.clamp(1, 100) as f32;
    match format {
        ImageFormat::Avif => (q * 0.9).max(1.0),
        _ => q,
    }
}

/// Quality used by „Hochwertig behalten“.
pub fn high_quality(format: ImageFormat) -> f32 {
    match format {
        ImageFormat::Avif => 90.0,
        _ => 95.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_as_json() {
        let s = RunSettings {
            images: Some(ImageSettings::Web(WebSettings {
                format: WebFormat::Avif,
                naming: Naming { prefix: "urlaub".into(), separator: Separator::Caps, ..Default::default() },
                ..Default::default()
            })),
            videos: Some(VideoSettings { format: VideoFormat::Webm, size: None, ..Default::default() }),
            audio: Some(AudioSettings { format: AudioFormat::Aac, content: AudioContent::Speech }),
            video_transcript: None,
            audio_transcript: Some(TranscriptSettings { timestamps: false }),
            pdfs: Some(PdfSettings {}),
            fonts: Some(FontSettings {}),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""mode":"web""#), "{json}");
        assert_eq!(serde_json::from_str::<RunSettings>(&json).unwrap(), s);

        let hq: RunSettings = serde_json::from_str(r#"{"images":{"mode":"high_quality","format":"png"}}"#).unwrap();
        assert_eq!(hq.images, Some(ImageSettings::HighQuality { format: ImageFormat::Png }));
        assert_eq!((hq.videos, hq.audio), (None, None), "settings from 2.1 still load");
        assert_eq!((hq.video_transcript, hq.audio_transcript), (None, None), "settings from 2.3 still load");
    }

    #[test]
    fn validates_ranges() {
        let mut w = WebSettings::default();
        let ok = RunSettings { images: Some(ImageSettings::Web(w.clone())), ..Default::default() };
        assert!(ok.validate().is_ok());
        w.max_side = 2_000_000;
        assert!(RunSettings { images: Some(ImageSettings::Web(w.clone())), ..Default::default() }.validate().is_err());
        w.max_side = 1400;
        w.quality = 0;
        assert!(RunSettings { images: Some(ImageSettings::Web(w)), ..Default::default() }.validate().is_err());
        let video = |size| RunSettings { videos: Some(VideoSettings { size, ..Default::default() }), ..Default::default() };
        assert!(video(None).validate().is_ok());
        assert!(video(Some(100)).validate().is_err());
        let both = RunSettings { videos: Some(VideoSettings::default()), video_transcript: Some(TranscriptSettings::default()), ..Default::default() };
        assert!(both.validate().is_err(), "a video is either converted or written down");
    }
}
