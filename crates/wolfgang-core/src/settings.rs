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

/// Fonts are converted 1:1 to WOFF2; nothing to configure yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FontSettings {}

/// Settings for one run. A pile with `None` is left alone.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RunSettings {
    pub images: Option<ImageSettings>,
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
            fonts: Some(FontSettings {}),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""mode":"web""#), "{json}");
        assert_eq!(serde_json::from_str::<RunSettings>(&json).unwrap(), s);

        let hq: RunSettings = serde_json::from_str(r#"{"images":{"mode":"high_quality","format":"png"}}"#).unwrap();
        assert_eq!(hq.images, Some(ImageSettings::HighQuality { format: ImageFormat::Png }));
    }

    #[test]
    fn validates_ranges() {
        let mut w = WebSettings::default();
        let ok = RunSettings { images: Some(ImageSettings::Web(w.clone())), fonts: None };
        assert!(ok.validate().is_ok());
        w.max_side = 2_000_000;
        assert!(RunSettings { images: Some(ImageSettings::Web(w.clone())), fonts: None }.validate().is_err());
        w.max_side = 1400;
        w.quality = 0;
        assert!(RunSettings { images: Some(ImageSettings::Web(w)), fonts: None }.validate().is_err());
    }
}
