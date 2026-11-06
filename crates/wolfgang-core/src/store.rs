//! The user's personal settings file, shared by CLI and app:
//! defaults, the last run („So wie letztes Mal“) and named presets.

use crate::error::{io_err, Error, Result};
use crate::naming::Separator;
use crate::settings::{ImageFormat, RunSettings, WebFormat, QUALITY_BALANCED, SIZE_NORMAL};
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
    pub sound: bool,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            web_format: WebFormat::Webp,
            max_side: SIZE_NORMAL,
            quality: QUALITY_BALANCED,
            hq_format: ImageFormat::Jpg,
            separator: Separator::Dash,
            sound: true,
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
    /// `$WOLFGANG_HOME/settings.json`, else `~/Library/Application Support/Wolfgang/settings.json`.
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
        store.last_run = Some(RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Png }), fonts: None });
        store.save_preset("Blog-Bilder", RunSettings::default()).unwrap();
        store.save(&path).unwrap();

        assert_eq!(UserStore::load(&path).unwrap().store, store);
    }

    #[test]
    fn presets_are_replaced_by_name_case_insensitively() {
        let mut s = UserStore::default();
        s.save_preset("Blog", RunSettings::default()).unwrap();
        s.save_preset("  blog ", RunSettings { fonts: Some(Default::default()), images: None }).unwrap();
        assert_eq!(s.presets.len(), 1);
        assert!(s.preset("BLOG").unwrap().settings.fonts.is_some());
        assert!(s.save_preset("   ", RunSettings::default()).is_err());
        assert!(s.remove_preset("blog"));
        assert!(s.presets.is_empty());
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
    }
}
