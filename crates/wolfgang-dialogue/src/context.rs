//! What Wolfgang knows about the drop, and the vocabulary (conditions and
//! placeholders) the dialogue file can use.

use std::collections::BTreeMap;
use std::path::PathBuf;
use wolfgang_core::kind::FileKind;
use wolfgang_core::settings::ImageSettings;
use wolfgang_core::{RunSettings, Summary, UserStore, Workspace};

/// Conditions usable in `when:` (prefix with `!` to negate).
pub const FLAGS: &[&str] = &[
    "has_images",
    "has_fonts",
    "has_unknown",
    "has_duplicates",
    "nothing_to_do",
    "only_fonts",
    "has_last_run",
    "has_presets",
    "has_known",
    "has_new",
    "keyword_files_found",
    "folders_without_keywords",
    "several_folders_without_keywords",
    "mode_web",
    "mode_hq",
    "from_template",
    "rerun_all",
    "failed_any",
    "all_failed",
    "converted_any",
    "has_notes",
    "saved_preset",
];

/// Placeholders holding numbers (usable as `{name:Einzahl:Mehrzahl}`).
pub const NUMBER_VARS: &[&str] = &[
    "images",
    "fonts",
    "unknown",
    "duplicates",
    "new_images",
    "known_images",
    "new_fonts",
    "known_fonts",
    "new_files",
    "known_files",
    "keyword_folders",
    "folders_without_keywords",
    "converted",
    "failed",
    "saved_percent",
];

/// Placeholders holding text.
pub const TEXT_VARS: &[&str] = &[
    "last_run",
    "summary",
    "output_folder",
    "default_web_format",
    "preview_dash",
    "preview_underscore",
    "preview_caps",
    "bytes_in",
    "bytes_out",
    "duration",
    "first_error",
    "save_preset_name",
];

/// Everything the conversation needs to know about the drop and the user.
#[derive(Debug, Clone)]
pub struct Context {
    pub summary: Summary,
    pub output_root: PathBuf,
    pub store: UserStore,
    /// Keyword words found in `*.txt` files, per relative folder.
    pub keyword_words: BTreeMap<String, Vec<String>>,
    /// First image (relative folder, file stem), for name previews.
    pub sample_image: Option<(String, String)>,
}

impl Context {
    pub fn new(ws: &Workspace, store: &UserStore) -> Self {
        Self {
            summary: ws.summary(),
            output_root: ws.output_root.clone(),
            store: store.clone(),
            keyword_words: ws.scan.keyword_files.iter().map(|(d, k)| (d.clone(), k.words.clone())).collect(),
            sample_image: ws.scan.of_kind(FileKind::Image).next().map(|f| {
                (f.rel_dir(), f.rel.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
            }),
        }
    }

    pub fn has_images(&self) -> bool {
        self.summary.images.total > 0
    }

    pub fn has_fonts(&self) -> bool {
        self.summary.fonts.total > 0
    }

    /// A last run is only offered if it actually configured images.
    pub fn last_run(&self) -> Option<&RunSettings> {
        self.store.last_run.as_ref().filter(|r| r.images.is_some())
    }
}

/// `41,7 MB`, `312 KB`
pub fn format_bytes(b: u64) -> String {
    if b < 1_000_000 {
        format!("{} KB", b.div_ceil(1000))
    } else {
        format!("{:.1} MB", b as f64 / 1e6).replace('.', ",")
    }
}

/// `0,8 Sekunden`, `12 Sekunden`, `3 Minuten`
pub fn format_duration(d: std::time::Duration) -> String {
    let s = d.as_secs_f64();
    if s < 10.0 {
        format!("{s:.1} Sekunden").replace('.', ",")
    } else if s < 120.0 {
        format!("{} Sekunden", s.round())
    } else {
        format!("{} Minuten", (s / 60.0).round())
    }
}

/// Short description of image settings, e.g. „fürs Web als WebP, 1400 px, Qualität 80“.
pub fn describe_images(s: &ImageSettings) -> String {
    match s {
        ImageSettings::HighQuality { format } => format!("hochwertig als {}", format.label()),
        ImageSettings::Web(w) => {
            let f: wolfgang_core::ImageFormat = w.format.into();
            let mut t = format!("fürs Web als {}, {} px, Qualität {}", f.label(), w.max_side, w.quality);
            if !w.naming.prefix.is_empty() {
                t.push_str(&format!(", Name „{}“", w.naming.prefix));
            }
            t
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn german_formatting() {
        assert_eq!(format_bytes(41_700_000), "41,7 MB");
        assert_eq!(format_bytes(312_400), "313 KB");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1,5 Sekunden");
        assert_eq!(format_duration(Duration::from_secs(45)), "45 Sekunden");
        assert_eq!(format_duration(Duration::from_secs(600)), "10 Minuten");
    }
}
