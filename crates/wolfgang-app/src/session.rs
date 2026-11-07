//! What happens in Wolfgang's office, independent of Tauri (so it can be tested):
//! a drop starts a conversation, answers move it on, a run converts, and
//! whatever should be remembered is saved when the conversation ends.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use wolfgang_core::run::Report;
use wolfgang_core::store::Defaults;
use wolfgang_core::{Plan, UserStore, Workspace};
use wolfgang_dialogue::context::describe_images;
use wolfgang_dialogue::{Context, Conversation, Script, Step};

/// A step plus what the UI needs around it.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    #[serde(flatten)]
    pub step: Step,
    /// After a run: the output folder, for „Im Finder zeigen“.
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetView {
    pub name: String,
    pub description: String,
}

struct Session {
    ws: Workspace,
    conv: Conversation,
    output: Option<PathBuf>,
    remembered: bool,
}

pub struct Office {
    store_path: PathBuf,
    pub store: UserStore,
    script: Arc<Script>,
    session: Option<Session>,
    rng: fastrand::Rng,
    pub running: bool,
}

impl Office {
    pub fn new(store_path: PathBuf, store: UserStore, script: Arc<Script>) -> Self {
        Self { store_path, store, script, session: None, rng: fastrand::Rng::new(), running: false }
    }

    /// Something landed on the desk: look at it and start talking.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>) -> Result<View, String> {
        if self.running {
            return Err("Moment, ich bin noch mit dem letzten Stapel beschäftigt.".into());
        }
        if let Some(missing) = paths.iter().find(|p| !p.exists()) {
            return Err(format!("„{}“ find ich nicht.", missing.display()));
        }
        if paths.is_empty() {
            return Err("Da war ja gar nix drin.".into());
        }
        let ws = Workspace::open(&paths).map_err(|e| e.to_string())?;
        let conv = Conversation::new(self.script.clone(), Context::new(&ws, &self.store), self.rng.u64(..));
        self.session = Some(Session { ws, conv, output: None, remembered: false });
        Ok(self.view())
    }

    pub fn choose(&mut self, index: usize) -> Result<View, String> {
        self.session_mut()?.conv.choose(index);
        Ok(self.view())
    }

    pub fn type_answer(&mut self, text: &str) -> Result<View, String> {
        self.session_mut()?.conv.type_answer(text);
        Ok(self.view())
    }

    pub fn back(&mut self) -> Result<View, String> {
        self.session_mut()?.conv.back();
        Ok(self.view())
    }

    /// The conversation reached „Los geht's!“: the concrete jobs.
    pub fn begin_run(&mut self) -> Result<Plan, String> {
        let s = self.session_mut()?;
        let Step::Start { settings, rerun, .. } = s.conv.step().clone() else {
            return Err("Wir sind noch nicht so weit.".into());
        };
        let plan = s.ws.plan(&settings, rerun).map_err(|e| e.to_string())?;
        s.output = Some(plan.output_root.clone());
        self.running = true;
        Ok(plan)
    }

    pub fn finished(&mut self, report: &Report) -> Result<View, String> {
        self.running = false;
        let s = self.session_mut()?;
        if report.converted == 0 {
            s.output = None;
        }
        s.conv.finished(report);
        Ok(self.view())
    }

    pub fn working_line(&mut self) -> String {
        match self.session.as_mut() {
            Some(s) => s.conv.working_line(),
            None => String::new(),
        }
    }

    pub fn idle_line(&mut self) -> String {
        Conversation::idle_line(&self.script, &mut self.rng)
    }

    pub fn defaults(&self) -> Defaults {
        self.store.defaults.clone()
    }

    pub fn set_defaults(&mut self, d: Defaults) -> Result<Defaults, String> {
        if !wolfgang_core::settings::MAX_SIDE_RANGE.contains(&d.max_side) || !(1..=100).contains(&d.quality) {
            return Err("Die Werte passen nicht.".into());
        }
        self.store.defaults = d;
        self.save()?;
        Ok(self.defaults())
    }

    /// Remembers the window size chosen with ⌘+ / ⌘− (`None` = default).
    pub fn set_window_scale(&mut self, scale: Option<u32>) -> Result<(), String> {
        self.store.defaults.window_scale = scale;
        self.save()
    }

    pub fn presets(&self) -> Vec<PresetView> {
        self.store
            .presets
            .iter()
            .map(|p| PresetView {
                name: p.name.clone(),
                description: p.settings.images.as_ref().map(describe_images).unwrap_or_else(|| "nur Schriften".into()),
            })
            .collect()
    }

    pub fn remove_preset(&mut self, name: &str) -> Result<Vec<PresetView>, String> {
        self.store.remove_preset(name);
        self.save()?;
        Ok(self.presets())
    }

    fn save(&self) -> Result<(), String> {
        self.store.save(&self.store_path).map_err(|e| e.to_string())
    }

    fn session_mut(&mut self) -> Result<&mut Session, String> {
        self.session.as_mut().ok_or_else(|| "Leg mir erst was auf den Tisch.".to_string())
    }

    /// The current step. When the conversation is over, its outcome
    /// („letztes Mal“, a new preset) is remembered exactly once.
    fn view(&mut self) -> View {
        let s = self.session.as_mut().expect("view() only with a session");
        let step = s.conv.step().clone();
        if matches!(step, Step::End { .. } | Step::Cancelled { .. }) && !s.remembered {
            s.remembered = true;
            s.conv.remember(&mut self.store);
            let _ = self.store.save(&self.store_path);
        }
        View { step, output: s.output.clone() }
    }
}

/// What the settings panel edits; mirrors `Defaults` for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct About {
    pub version: String,
    pub cli_installed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use wolfgang_core::image::{self, Pixels};
    use wolfgang_core::ImageFormat;

    fn office(t: &std::path::Path) -> Office {
        Office::new(t.join("home/settings.json"), UserStore::default(), wolfgang_dialogue::builtin())
    }

    fn pick(o: &mut Office, view: &View, part: &str) -> View {
        let Step::Ask { answers, .. } = &view.step else { panic!("{view:?}") };
        let i = answers.iter().position(|a| a.label.contains(part)).unwrap_or_else(|| panic!("no '{part}' in {answers:?}"));
        o.choose(i).unwrap()
    }

    #[test]
    fn a_whole_visit() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("Bilder");
        std::fs::create_dir_all(&dir).unwrap();
        let px = Pixels { width: 300, height: 200, rgba: vec![90; 300 * 200 * 4], icc: None };
        std::fs::write(dir.join("a.jpg"), image::encode(&px, ImageFormat::Jpg, 80.0).unwrap()).unwrap();

        let mut o = office(t.path());
        assert!(o.choose(0).is_err(), "nothing on the desk yet");
        assert!(o.drop_files(vec![t.path().join("nope")]).is_err());

        let v = o.drop_files(vec![dir.clone()]).unwrap();
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["type"], "ask", "step fields are flattened: {json}");
        let v = pick(&mut o, &v, "Hochwertig");
        let v = pick(&mut o, &v, "PNG");
        let v = pick(&mut o, &v, "Los");
        assert!(matches!(v.step, Step::Start { .. }));

        let plan = o.begin_run().unwrap();
        assert!(o.running);
        assert!(o.drop_files(vec![dir.clone()]).is_err(), "no new drop while working");
        let report = wolfgang_core::execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
        let v = o.finished(&report).unwrap();
        assert!(!o.running);
        assert_eq!(v.output, Some(t.path().canonicalize().unwrap().join("Bilder-wolfgang")));
        assert!(t.path().join("Bilder-wolfgang/a.png").exists());

        let v = pick(&mut o, &v, "Danke");
        assert!(matches!(v.step, Step::End { .. }));
        let saved = UserStore::load(&t.path().join("home/settings.json")).unwrap().store;
        assert!(saved.last_run.is_some(), "remembered on the way out");
    }

    #[test]
    fn settings_and_presets() {
        let t = tempfile::tempdir().unwrap();
        let mut o = office(t.path());
        let mut d = o.defaults();
        d.quality = 70;
        assert_eq!(o.set_defaults(d.clone()).unwrap().quality, 70);
        d.quality = 0;
        assert!(o.set_defaults(d).is_err());
        o.store.save_preset("Blog", wolfgang_core::RunSettings::default()).unwrap();
        assert_eq!(o.presets()[0].description, "nur Schriften");
        assert!(o.remove_preset("blog").unwrap().is_empty());
        assert_eq!(UserStore::load(&t.path().join("home/settings.json")).unwrap().store.defaults.quality, 70);
    }
}
