//! Sorting files into piles by extension.

use serde::{Deserialize, Serialize};
use std::path::Path;

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "heic", "heif", "gif", "tif", "tiff", "avif", // raster
    "cr2", "cr3", "nef", "arw", "dng", // camera RAW
];
const FONT_EXTS: &[&str] = &["ttf", "otf", "woff"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Image,
    Font,
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
        assert_eq!(classify(Path::new("doc.pdf")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new("font.woff2")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new("noext")), Some(FileKind::Unknown));
        assert_eq!(classify(Path::new(".DS_Store")), None);
    }
}
