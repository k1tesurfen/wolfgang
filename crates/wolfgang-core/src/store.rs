//! The user's personal settings file, shared by CLI and app:
//! defaults, the last run („So wie letztes Mal“) and named presets.

use crate::error::{io_err, Error, Result};
use crate::naming::Separator;
use crate::settings::{AudioFormat, ImageFormat, RunSettings, VideoFormat, VideoQuality, WebFormat, QUALITY_BALANCED, SIZE_NORMAL, VIDEO_NORMAL};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Overrides the store location (tests, portable setups).
pub const HOME_ENV: &str = "WOLFGANG_HOME";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub web_format: WebFormat,
    pub max_side: u32,
    pub quality: u8,
    pub hq_format: ImageFormat,
    pub separator: Separator,
    pub video_format: VideoFormat,
    /// Shorter side of videos; `None` = original size.
    pub video_size: Option<u32>,
    pub video_quality: VideoQuality,
    pub audio_format: AudioFormat,
    pub sound: bool,
    /// App window size as a whole multiple of the scene; `None` = the app's default.
    pub window_scale: Option<u32>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            web_format: WebFormat::Webp,
            max_side: SIZE_NORMAL,
            quality: QUALITY_BALANCED,
            hq_format: ImageFormat::Jpg,
            separator: Separator::Dash,
            video_format: VideoFormat::Mp4,
            video_size: Some(VIDEO_NORMAL),
            video_quality: VideoQuality::Balanced,
            audio_format: AudioFormat::Mp3,
            sound: true,
            window_scale: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub settings: RunSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserStore {
    pub defaults: Defaults,
    pub last_run: Option<RunSettings>,
    pub presets: Vec<Preset>,
}

/// Result of loading: the store plus a note if a broken file had to be set aside.
pub struct Loaded {
    pub store: UserStore,
    pub recovered_from: Option<PathBuf>,
}

impl UserStore {
    /// `$WOLFGANG_HOME/settings.json`, else `~/Library/Application Support/Wolfgang/settings.json`
    /// (the same for Wolfgang, Ully and Burgunde: they share settings, presets and the add-on).
    pub fn default_path() -> PathBuf {
        let dir = std::env::var_os(HOME_ENV)
            .map(PathBuf::from)
            .or_else(|| dirs::config_dir().map(|d| d.join("Wolfgang")))
            .unwrap_or_else(|| PathBuf::from(".wolfgang"));
        dir.join("settings.json")
    }

    /// Loads the store. A missing file gives defaults; an unreadable one is
    /// renamed to `settings.broken-<timestamp>.json` so the user never gets stuck.
    pub fn load(path: &Path) -> Result<Loaded> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Loaded { store: Self::default(), recovered_from: None }),
            Err(e) => return Err(io_err(path)(e)),
        };
        match serde_json::from_str(&text) {
            Ok(store) => Ok(Loaded { store, recovered_from: None }),
            Err(_) => {
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                let backup = path.with_file_name(format!("settings.broken-{stamp}.json"));
                std::fs::rename(path, &backup).map_err(io_err(path))?;
                Ok(Loaded { store: Self::default(), recovered_from: Some(backup) })
            }
        }
    }

    /// Writes atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| Error::Settings(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(io_err(&tmp))?;
        std::fs::rename(&tmp, path).map_err(io_err(path))
    }

    /// Remembers a run as „letztes Mal“ **per pile**: a run with only videos
    /// doesn't forget how the photos were done last time.
    pub fn remember_run(&mut self, s: &RunSettings) {
        if s.images.is_none() && s.videos.is_none() && s.audio.is_none() && s.video_transcript.is_none() && s.audio_transcript.is_none() && s.pdfs.is_none() {
            return;
        }
        let last = self.last_run.get_or_insert_with(RunSettings::default);
        if s.images.is_some() {
            last.images = s.images.clone();
        }
        // Converted or written down: whichever it was this time replaces the other.
        if s.videos.is_some() || s.video_transcript.is_some() {
            last.videos = s.videos.clone();
            last.video_transcript = s.video_transcript;
        }
        if s.audio.is_some() || s.audio_transcript.is_some() {
            last.audio = s.audio;
            last.audio_transcript = s.audio_transcript;
        }
        if s.pdfs.is_some() {
            last.pdfs = s.pdfs;
        }
        last.fonts = None;
    }

    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Adds a preset, replacing one with the same name (case-insensitive).
    pub fn save_preset(&mut self, name: &str, settings: RunSettings) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Settings("Die Vorlage braucht einen Namen".into()));
        }
        self.remove_preset(name);
        self.presets.push(Preset { name: name.to_owned(), settings });
        Ok(())
    }

    pub fn remove_preset(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|p| !p.name.eq_ignore_ascii_case(name.trim()));
        self.presets.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ImageSettings;

    #[test]
    fn missing_file_gives_defaults_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/settings.json");
        let mut store = UserStore::load(&path).unwrap().store;
        assert_eq!(store, UserStore::default());

        store.defaults.web_format = WebFormat::Avif;
        store.last_run = Some(RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Png }), ..Default::default() });
        store.save_preset("Blog-Bilder", RunSettings::default()).unwrap();
        store.save(&path).unwrap();

        assert_eq!(UserStore::load(&path).unwrap().store, store);
    }

    #[test]
    fn presets_are_replaced_by_name_case_insensitively() {
        let mut s = UserStore::default();
        s.save_preset("Blog", RunSettings::default()).unwrap();
        s.save_preset("  blog ", RunSettings { fonts: Some(Default::default()), ..Default::default() }).unwrap();
        assert_eq!(s.presets.len(), 1);
        assert!(s.preset("BLOG").unwrap().settings.fonts.is_some());
        assert!(s.save_preset("   ", RunSettings::default()).is_err());
        assert!(s.remove_preset("blog"));
        assert!(s.presets.is_empty());
    }

    #[test]
    fn last_run_is_remembered_per_pile() {
        use crate::settings::{AudioSettings, VideoSettings};
        let mut s = UserStore::default();
        s.remember_run(&RunSettings { fonts: Some(Default::default()), ..Default::default() });
        assert_eq!(s.last_run, None, "fonts alone have nothing to remember");
        let photos = Some(ImageSettings::HighQuality { format: ImageFormat::Png });
        s.remember_run(&RunSettings { images: photos.clone(), fonts: Some(Default::default()), ..Default::default() });
        s.remember_run(&RunSettings { videos: Some(VideoSettings::default()), ..Default::default() });
        s.remember_run(&RunSettings { audio: Some(AudioSettings::default()), ..Default::default() });
        let last = s.last_run.clone().unwrap();
        assert_eq!(last.images, photos, "videos and audio didn't erase the photos");
        assert!(last.videos.is_some() && last.audio.is_some() && last.fonts.is_none());

        use crate::settings::TranscriptSettings;
        s.remember_run(&RunSettings { audio_transcript: Some(TranscriptSettings::default()), ..Default::default() });
        let last = s.last_run.clone().unwrap();
        assert!(last.audio.is_none() && last.audio_transcript.is_some(), "the meeting replaces the music");
        assert!(last.videos.is_some() && last.images.is_some());
    }

    #[test]
    fn broken_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ kaputt").unwrap();
        let loaded = UserStore::load(&path).unwrap();
        assert_eq!(loaded.store, UserStore::default());
        let backup = loaded.recovered_from.unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "{ kaputt");
        assert!(!path.exists());
    }

    #[test]
    fn unknown_fields_and_partial_files_are_tolerated() {
        let s: UserStore = serde_json::from_str(r#"{"defaults":{"quality":70},"future_field":1}"#).unwrap();
        assert_eq!(s.defaults.quality, 70);
        assert_eq!(s.defaults.max_side, SIZE_NORMAL);
        assert_eq!(s.defaults.video_size, Some(VIDEO_NORMAL), "settings from 2.2 get the video defaults");
        let original: UserStore = serde_json::from_str(r#"{"defaults":{"video_size":null}}"#).unwrap();
        assert_eq!(original.defaults.video_size, None, "null = original size");
    }
}
