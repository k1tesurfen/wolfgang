//! The settings being filled in during a conversation. Answers in the dialogue
//! file write to it with `set: { key: value }`.

use crate::script::Value;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wolfgang_core::settings::{FontSettings, ImageFormat, ImageSettings, RunSettings, WebSettings};
use wolfgang_core::store::{Defaults, UserStore};
use wolfgang_core::RerunPolicy;

/// Every key the dialogue file may write.
pub const KEYS: &[&str] = &[
    "mode",
    "web.format",
    "web.max_side",
    "web.quality",
    "hq.format",
    "naming.prefix",
    "naming.use_keyword_files",
    "naming.keywords_all",
    "naming.folder_keywords",
    "naming.separator",
    "naming.append_original",
    "rerun",
    "use_last",
    "preset",
    "save_preset",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Web,
    Hq,
}

/// Where the settings came from, if not answered one by one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
    LastRun,
    Preset(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub mode: Option<Mode>,
    pub web: WebSettings,
    pub hq_format: ImageFormat,
    pub rerun: RerunPolicy,
    pub template: Option<Template>,
    pub save_preset: Option<String>,
}

impl Draft {
    /// Starts from the user's defaults, so their usual choices are pre-marked.
    pub fn new(d: &Defaults) -> Self {
        let mut web = WebSettings { format: d.web_format, max_side: d.max_side, quality: d.quality, ..Default::default() };
        web.naming.separator = d.separator;
        web.naming.use_keyword_files = true;
        Self { mode: None, web, hq_format: d.hq_format, rerun: RerunPolicy::OnlyNew, template: None, save_preset: None }
    }

    /// Applies one `key: value`. `folder` is the current item of a `foreach` node.
    pub fn apply(&mut self, key: &str, value: &Value, store: &UserStore, folder: Option<&str>) -> Result<(), String> {
        let n = &mut self.web.naming;
        match key {
            "mode" => self.mode = Some(typed(value)?),
            "web.format" => self.web.format = typed(value)?,
            "web.max_side" => self.web.max_side = typed(value)?,
            "web.quality" => self.web.quality = typed(value)?,
            "hq.format" => self.hq_format = typed(value)?,
            "naming.prefix" => n.prefix = typed::<String>(value)?.trim().to_owned(),
            "naming.use_keyword_files" => n.use_keyword_files = typed(value)?,
            "naming.keywords_all" => n.keywords_all = typed::<String>(value)?.trim().to_owned(),
            "naming.folder_keywords" => {
                let folder = folder.ok_or("naming.folder_keywords geht nur in einem foreach-Knoten")?;
                n.folder_keywords.insert(folder.to_owned(), typed::<String>(value)?.trim().to_owned());
            }
            "naming.separator" => n.separator = typed(value)?,
            "naming.append_original" => n.append_original = typed(value)?,
            "rerun" => self.rerun = typed(value)?,
            "use_last" => {
                if typed::<bool>(value)? {
                    let last = store.last_run.clone().ok_or("es gibt kein letztes Mal")?;
                    self.adopt(&last);
                    self.template = Some(Template::LastRun);
                } else {
                    self.template = None; // answering one by one from here
                }
            }
            "preset" => {
                let name: String = typed(value)?;
                let preset = store.preset(&name).ok_or_else(|| format!("Vorlage '{name}' gibt es nicht"))?;
                self.adopt(&preset.settings.clone());
                self.template = Some(Template::Preset(preset.name.clone()));
            }
            "save_preset" => self.save_preset = Some(typed::<String>(value)?.trim().to_owned()).filter(|s| !s.is_empty()),
            other => return Err(format!("unbekannte Einstellung '{other}'")),
        }
        Ok(())
    }

    /// Current value of a key, used to mark the answer matching it.
    pub fn get(&self, key: &str) -> Option<Value> {
        let n = &self.web.naming;
        let v = match key {
            "mode" => serde_norway::to_value(self.mode?),
            "web.format" => serde_norway::to_value(self.web.format),
            "web.max_side" => serde_norway::to_value(self.web.max_side),
            "web.quality" => serde_norway::to_value(self.web.quality),
            "hq.format" => serde_norway::to_value(self.hq_format),
            "naming.separator" => serde_norway::to_value(n.separator),
            "naming.append_original" => serde_norway::to_value(n.append_original),
            "naming.use_keyword_files" => serde_norway::to_value(n.use_keyword_files),
            "rerun" => serde_norway::to_value(self.rerun),
            _ => return None,
        };
        v.ok()
    }

    /// Takes over a previous run's or preset's choices. Typed keywords are
    /// dropped: they describe *those* pictures, not the ones on the desk now.
    fn adopt(&mut self, s: &RunSettings) {
        match &s.images {
            Some(ImageSettings::HighQuality { format }) => {
                self.mode = Some(Mode::Hq);
                self.hq_format = *format;
            }
            Some(ImageSettings::Web(w)) => {
                self.mode = Some(Mode::Web);
                self.web = w.clone();
                self.web.naming.keywords_all.clear();
                self.web.naming.folder_keywords.clear();
            }
            None => {}
        }
    }

    pub fn settings(&self, has_images: bool, has_fonts: bool) -> RunSettings {
        let images = match (has_images, self.mode) {
            (true, Some(Mode::Web)) => Some(ImageSettings::Web(self.web.clone())),
            (true, Some(Mode::Hq)) => Some(ImageSettings::HighQuality { format: self.hq_format }),
            _ => None,
        };
        RunSettings { images, fonts: has_fonts.then(FontSettings::default) }
    }
}

fn typed<T: DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_norway::from_value(v.clone()).map_err(|e| format!("Wert {v:?} passt nicht: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wolfgang_core::naming::Separator;
    use wolfgang_core::WebFormat;

    fn v(yaml: &str) -> Value {
        serde_norway::from_str(yaml).unwrap()
    }

    #[test]
    fn applies_typed_values() {
        let store = UserStore::default();
        let mut d = Draft::new(&store.defaults);
        d.apply("mode", &v("web"), &store, None).unwrap();
        d.apply("web.format", &v("avif"), &store, None).unwrap();
        d.apply("web.max_side", &v("800"), &store, None).unwrap();
        d.apply("naming.separator", &v("caps"), &store, None).unwrap();
        d.apply("naming.folder_keywords", &v("'rote Schuhe '"), &store, Some("Schuhe")).unwrap();
        assert_eq!((d.web.format, d.web.max_side, d.web.naming.separator), (WebFormat::Avif, 800, Separator::Caps));
        assert_eq!(d.web.naming.folder_keywords["Schuhe"], "rote Schuhe");
        assert!(d.apply("web.format", &v("gif"), &store, None).is_err());
        assert!(d.apply("web.max_side", &v("-3"), &store, None).is_err());
        assert!(d.apply("naming.folder_keywords", &v("x"), &store, None).is_err());
    }

    #[test]
    fn last_run_and_presets_are_adopted_without_old_keywords() {
        let mut store = UserStore::default();
        let mut w = WebSettings { max_side: 2400, ..Default::default() };
        w.naming.prefix = "kunde".into();
        w.naming.keywords_all = "alte Stichworte".into();
        store.last_run = Some(RunSettings { images: Some(ImageSettings::Web(w)), fonts: None });
        store.save_preset("Druck", RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Png }), fonts: None }).unwrap();

        let mut d = Draft::new(&store.defaults);
        d.apply("use_last", &v("true"), &store, None).unwrap();
        assert_eq!((d.mode, d.web.max_side, d.web.naming.prefix.as_str()), (Some(Mode::Web), 2400, "kunde"));
        assert!(d.web.naming.keywords_all.is_empty());
        assert_eq!(d.template, Some(Template::LastRun));

        d.apply("preset", &v("druck"), &store, None).unwrap();
        assert_eq!((d.mode, d.hq_format), (Some(Mode::Hq), ImageFormat::Png));
        assert_eq!(d.template, Some(Template::Preset("Druck".into())));
        assert!(d.apply("preset", &v("gibtsnicht"), &store, None).is_err());
    }

    #[test]
    fn builds_run_settings_per_pile() {
        let store = UserStore::default();
        let mut d = Draft::new(&store.defaults);
        assert_eq!(d.settings(true, true).images, None, "no mode chosen yet");
        d.apply("mode", &v("hq"), &store, None).unwrap();
        let s = d.settings(true, false);
        assert!(matches!(s.images, Some(ImageSettings::HighQuality { .. })));
        assert!(s.fonts.is_none());
        assert!(d.settings(false, true).images.is_none());
    }
}
