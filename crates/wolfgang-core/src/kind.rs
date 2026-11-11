//! Sorting files into piles by extension.

use serde::{Deserialize, Serialize};
use std::path::Path;

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "heic", "heif", "gif", "tif", "tiff", "avif", // raster
    "cr2", "cr3", "nef", "arw", "dng", // camera RAW
];
const FONT_EXTS: &[&str] = &["ttf", "otf", "woff"];
const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v", "mkv", "webm", "avi", "mts", "m2ts", "mpg", "mpeg", "wmv", "3gp"];
const PDF_EXTS: &[&str] = &["pdf"];
const TABLE_EXTS: &[&str] = &["xlsx", "xlsm", "xls", "ods", "csv"];
const AUDIO_EXTS: &[&str] = &["wav", "aif", "aiff", "flac", "m4a", "mp3", "aac", "ogg", "opus", "caf", "wma"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Image,
    Font,
    /// Converted with the bundled ffmpeg.
    Video,
    Audio,
    /// Wolfgang pulls the pictures out.
    Pdf,
    /// Excel becomes CSV, CSV becomes Excel.
    Table,
    /// A `*.txt` next to images, holding SEO keywords.
    KeywordFile,
    /// Something Wolfgang can't handle (yet).
    Unknown,
}

/// Classifies a path. `None` means "ignore silently" (hidden files like `.DS_Store`).
pub fn classify(path: &Path) -> Option<FileKind> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with('.') {
        return None;
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    Some(match ext.as_deref() {
        Some(e) if IMAGE_EXTS.contains(&e) => FileKind::Image,
        Some(e) if FONT_EXTS.contains(&e) => FileKind::Font,
        Some(e) if VIDEO_EXTS.contains(&e) => FileKind::Video,
        Some(e) if AUDIO_EXTS.contains(&e) => FileKind::Audio,
        Some(e) if PDF_EXTS.contains(&e) => FileKind::Pdf,
        Some(e) if TABLE_EXTS.contains(&e) => FileKind::Table,
        Some("txt") => FileKind::KeywordFile,
        _ => FileKind::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension_case_insensitively() {
        assert_eq!(classify(Path::new("a/B.HEIC")), Some(FileKind::Image));
        assert_eq!(classify(Path::new("x.NEF")), Some(FileKind::Image));
        assert_eq!(classify(Path::new("Inter Bold.otf")), Some(FileKind::Font));
        assert_eq!(classify(Path::new("keywords.txt")), Some(FileKind::KeywordFile));
        assert_eq!(classify(Path::new("Drohne.MOV")), Some(FileKind::Video));
        assert_eq!(classify(Path::new("clip.webm")), Some(FileKind::Video));
        assert_eq!(classify(Path::new("Interview.WAV")), Some(FileKind::Audio));
        assert_eq!(classify(Path::new("anim.gif")), Some(FileKind::Image), "GIFs stay photos");
        assert_eq!(classify(Path::new("Broschüre.PDF")), Some(FileKind::Pdf));
        assert_eq!(classify(Path::new("Preise.XLSX")), Some(FileKind::Table));
        assert_eq!(classify(Path::new("export.csv")), Some(FileKind::Table));
        assert_eq!(classify(Path::new("alt.xls")), Some(FileKind::Table));
        assert_eq!(classify(Path::new("Verein.ods")), Some(FileKind::Table));
        assert_eq!(classify(Path::new("Zahlen.numbers")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new("doc.docx")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new("font.woff2")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new("noext")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new(".DS_Store")), None);
    }
}
