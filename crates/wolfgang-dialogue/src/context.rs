//! What Wolfgang knows about the drop, and the vocabulary (conditions and
//! placeholders) the dialogue file can use.

use std::collections::BTreeMap;
use std::path::PathBuf;
use wolfgang_core::kind::FileKind;
use wolfgang_core::settings::{AudioSettings, ImageSettings, VideoSettings};
use wolfgang_core::workspace::Piles;
use wolfgang_core::{RunSettings, Summary, UserStore, Workspace};

/// Conditions usable in `when:` (prefix with `!` to negate).
pub const FLAGS: &[&str] = &[
    "has_images",
    "has_videos",
    "has_audio",
    "has_fonts",
    "has_media",
    "only_images",
    "several_piles",
    "videos_have_sound",
    "media_tools_missing",
    "images_asked",
    "videos_asked",
    "audio_asked",
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
    "videos",
    "audio_files",
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
    "pile_list",
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

    pub fn piles(&self) -> Piles {
        self.summary.piles()
    }

    pub fn has_images(&self) -> bool {
        self.piles().images
    }

    pub fn has_fonts(&self) -> bool {
        self.piles().fonts
    }

    /// A last run is only offered if it configured a pile that's on the desk now.
    pub fn last_run(&self) -> Option<&RunSettings> {
        let p = self.piles();
        self.store.last_run.as_ref().filter(|r| (p.images && r.images.is_some()) || (p.videos && r.videos.is_some()) || (p.audio && r.audio.is_some()))
    }

    /// The last run, described for the piles on the desk.
    pub fn describe_last_run(&self) -> String {
        let p = self.piles();
        self.last_run().map(|r| describe_settings(r, p)).unwrap_or_default()
    }
}

/// „42 Fotos, 3 Videos und eine Schrift“
pub fn pile_list(s: &Summary) -> String {
    let p = s.piles();
    let mut parts = Vec::new();
    let mut add = |on: bool, n: usize, one: &str, many: &str| {
        if on {
            parts.push(if n == 1 { one.to_owned() } else { format!("{n} {many}") });
        }
    };
    add(p.images, s.images.total, "ein Foto", "Fotos");
    add(p.videos, s.videos.total, "ein Video", "Videos");
    add(p.audio, s.audio.total, "eine Audiodatei", "Audiodateien");
    add(p.fonts, s.fonts.total, "eine Schrift", "Schriften");
    join_and(&parts)
}

/// `a`, `a und b`, `a, b und c`
pub fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} und {last}", init.join(", ")),
    }
}

/// Describes the piles of a run (or preset); `on` limits it to piles that matter.
pub fn describe_settings(s: &RunSettings, on: Piles) -> String {
    let mut parts = Vec::new();
    let several = [on.images && s.images.is_some(), on.videos && s.videos.is_some(), on.audio && s.audio.is_some()].iter().filter(|b| **b).count() > 1;
    let label = |what: &str, text: String| if several { format!("{what} {text}") } else { text };
    if let Some(i) = s.images.as_ref().filter(|_| on.images) {
        parts.push(label("Fotos", describe_images(i)));
    }
    if let Some(v) = s.videos.as_ref().filter(|_| on.videos) {
        parts.push(label("Videos", describe_videos(v)));
    }
    if let Some(a) = s.audio.as_ref().filter(|_| on.audio) {
        parts.push(label("Audio", describe_audio(a)));
    }
    parts.join("; ")
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

/// Short description of image settings, e.g. „fürs Web als WebP, 1600 px, Qualität 80“.
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

/// „als MP4, 1080p, mit Ton und Vorschaubild“
pub fn describe_videos(v: &VideoSettings) -> String {
    let size = v.size.map_or("Originalgröße".to_owned(), |p| format!("{p}p"));
    let mut t = format!("als {}, {size}", v.format.label());
    if v.quality != wolfgang_core::VideoQuality::Balanced {
        t.push_str(&format!(", {}", v.quality.label()));
    }
    t.push_str(match (v.sound, v.poster) {
        (true, true) => ", mit Ton und Vorschaubild",
        (true, false) => ", mit Ton",
        (false, true) => ", ohne Ton, mit Vorschaubild",
        (false, false) => ", ohne Ton",
    });
    t
}

/// „als MP3 (Sprache)“
pub fn describe_audio(a: &AudioSettings) -> String {
    format!("als {} ({})", a.format.label(), a.content.label())
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
        assert_eq!(join_and(&["a".into(), "b".into(), "c".into()]), "a, b und c");
    }

    #[test]
    fn describes_videos_and_audio() {
        assert_eq!(describe_videos(&VideoSettings::default()), "als MP4, 1080p, mit Ton und Vorschaubild");
        let v = VideoSettings { format: wolfgang_core::VideoFormat::Webm, size: None, sound: false, poster: false, quality: wolfgang_core::VideoQuality::Nicer };
        assert_eq!(describe_videos(&v), "als WebM, Originalgröße, schöner, ohne Ton");
        let all = Piles { images: true, videos: true, audio: true, fonts: false };
        let s = RunSettings { videos: Some(VideoSettings::default()), audio: Some(AudioSettings::default()), ..Default::default() };
        assert_eq!(describe_settings(&s, all), "Videos als MP4, 1080p, mit Ton und Vorschaubild; Audio als MP3 (Musik)");
        assert_eq!(describe_settings(&s, Piles { audio: true, ..Default::default() }), "als MP3 (Musik)");
    }
}
