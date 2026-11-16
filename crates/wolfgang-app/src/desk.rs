//! „Normaler Modus“: the same engine without the conversation. A drop becomes a
//! description of its piles (what the cards need), the cards build the settings
//! themselves, and the run reports back in plain words instead of the character's.

use serde::Serialize;
use std::path::PathBuf;
use wolfgang_core::run::{Note, Report};
use wolfgang_core::settings::WebSettings;
use wolfgang_core::store::{Defaults, Preset};
use wolfgang_core::workspace::{PileSummary, RerunPolicy};
use wolfgang_core::{Plan, RunSettings, UserStore, Workspace};
use wolfgang_dialogue::context::{self, format_bytes, format_duration, format_estimate, Context};

/// One pile on the desk, as a card.
#[derive(Debug, Clone, Serialize)]
pub struct PileView {
    /// images | videos | audio | pdfs | tables | fonts
    pub key: &'static str,
    pub total: usize,
    /// Converted in an earlier run.
    pub known: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeywordFolder {
    /// Relative folder („“ = the dropped folder itself).
    pub dir: String,
    pub words: String,
}

/// Everything the cards need to know about a drop.
#[derive(Debug, Clone, Serialize)]
pub struct DeskView {
    pub piles: Vec<PileView>,
    /// Tables: how many of them are CSV (they become Excel; the rest become CSV).
    pub csv_tables: usize,
    pub known: usize,
    pub fresh: usize,
    /// Files nobody here can do anything with (relative names).
    pub unknown: Vec<String>,
    pub duplicates: usize,
    pub keyword_folders: Vec<KeywordFolder>,
    pub folders_without_keywords: Vec<String>,
    /// The folder a top-level dir („“) stands for.
    pub root_name: String,
    pub videos_with_sound: usize,
    pub animated_gifs: usize,
    pub transparent_gifs: usize,
    /// There are videos or audio, but the bundled ffmpeg is missing.
    pub media_tools_missing: bool,
    /// The add-on for transcripts is installed.
    pub transcription: bool,
    /// `Urlaub/wolfgang_output`
    pub output_folder: String,
    pub defaults: Defaults,
    /// „Wie letztes Mal“, if it covers a pile on the desk.
    pub last_run: Option<RunSettings>,
    pub presets: Vec<Preset>,
}

/// What came out of a run, in plain words.
#[derive(Debug, Clone, Serialize)]
pub struct ResultView {
    pub converted: usize,
    pub cancelled: bool,
    /// „41,7 MB → 1,2 MB (98 % kleiner)“, when sizes compare.
    pub saved: Option<String>,
    pub duration: String,
    /// Further lines: PDF pictures, transcripts, tables, notes.
    pub lines: Vec<String>,
    /// (file, what went wrong)
    pub failed: Vec<(String, String)>,
    /// For „Im Finder zeigen“; `None` if nothing was written.
    pub output: Option<PathBuf>,
    pub output_folder: String,
}

pub struct Desk {
    pub paths: Vec<PathBuf>,
    ws: Workspace,
    ctx: Context,
    /// The settings of the run that's going (remembered when it ends).
    running: Option<RunSettings>,
}

impl Desk {
    /// Looks at a drop. `before` are the paths already on the desk (a second drop adds to them).
    pub fn open(paths: Vec<PathBuf>, before: Option<&Desk>, store: &UserStore) -> Result<Self, String> {
        if let Some(missing) = paths.iter().find(|p| !p.exists()) {
            return Err(format!("„{}“ gibt es nicht.", missing.display()));
        }
        let mut all: Vec<PathBuf> = before.map(|d| d.paths.clone()).unwrap_or_default();
        for p in paths {
            if !all.contains(&p) {
                all.push(p);
            }
        }
        if all.is_empty() {
            return Err("Da war nichts drin.".into());
        }
        let ws = Workspace::open(&all).map_err(|e| e.to_string())?;
        let ctx = Context::new(&ws, store);
        Ok(Self { paths: all, ws, ctx, running: None })
    }

    pub fn view(&self) -> DeskView {
        let s = &self.ctx.summary;
        let p = s.piles();
        let pile = |key, on: bool, ps: &PileSummary| on.then_some(PileView { key, total: ps.total, known: ps.known });
        let piles: Vec<PileView> = [
            pile("images", p.images, &s.images),
            pile("videos", p.videos, &s.videos),
            pile("audio", p.audio, &s.audio),
            pile("pdfs", p.pdfs, &s.pdfs),
            pile("tables", p.tables, &s.tables),
            pile("fonts", p.fonts, &s.fonts),
        ]
        .into_iter()
        .flatten()
        .collect();
        let known = piles.iter().map(|p| p.known).sum();
        let total: usize = piles.iter().map(|p| p.total).sum();
        let root = self.paths.first().filter(|_| self.paths.len() == 1 && self.paths[0].is_dir());
        DeskView {
            csv_tables: s.csv_tables.total,
            known,
            fresh: total - known,
            unknown: s.unknown.iter().map(|u| u.display().to_string()).collect(),
            duplicates: s.duplicates,
            keyword_folders: self.ctx.keyword_words.iter().map(|(dir, w)| KeywordFolder { dir: dir.clone(), words: w.join(" ") }).collect(),
            folders_without_keywords: s.folders_without_keywords.clone(),
            root_name: root.and_then(|r| r.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "oberste Ebene".into()),
            videos_with_sound: s.videos_with_sound,
            animated_gifs: s.animated_gifs,
            transparent_gifs: s.transparent_gifs,
            media_tools_missing: s.media_tools_missing,
            transcription: self.ctx.transcription,
            output_folder: self.output_folder(),
            defaults: self.ctx.store.defaults.clone(),
            last_run: self.ctx.last_run().cloned(),
            presets: self.ctx.store.presets.clone(),
            piles,
        }
    }

    /// The first image's new name with these web settings.
    pub fn preview(&self, web: &WebSettings) -> String {
        context::name_preview(&self.ctx, web, web.naming.separator)
    }

    /// „etwa 9 Minuten“ when the run takes a minute or more.
    pub fn estimate(&self, settings: &RunSettings, again: bool) -> Option<String> {
        context::estimate(&self.ctx.summary, settings, !again).filter(|d| d.as_secs() >= 60).map(format_estimate)
    }

    /// Only what's on the desk: a template may know piles that aren't here.
    fn on_desk(&self, mut s: RunSettings) -> RunSettings {
        let p = self.ctx.summary.piles();
        if !p.images {
            s.images = None;
        }
        if !p.videos {
            (s.videos, s.video_transcript) = (None, None);
        }
        if !p.audio {
            (s.audio, s.audio_transcript) = (None, None);
        }
        if !p.pdfs {
            s.pdfs = None;
        }
        s.tables = p.tables.then(Default::default);
        s.fonts = p.fonts.then(Default::default);
        s
    }

    pub fn plan(&mut self, settings: RunSettings, again: bool) -> Result<Plan, String> {
        let settings = self.on_desk(settings);
        settings.validate().map_err(|e| e.to_string())?;
        if (settings.video_transcript.is_some() || settings.audio_transcript.is_some()) && !wolfgang_core::transcribe::installed() {
            return Err("Fürs Aufschreiben fehlt das Zusatzmodul. Es lässt sich in den Einstellungen installieren.".into());
        }
        let plan = self.ws.plan(&settings, if again { RerunPolicy::All } else { RerunPolicy::OnlyNew }).map_err(|e| e.to_string())?;
        if plan.jobs.is_empty() {
            return Err("Es gibt nichts zu tun: alles ist schon konvertiert.".into());
        }
        self.running = Some(settings);
        Ok(plan)
    }

    /// The run ended: remember it („wie letztes Mal“) and describe it.
    pub fn finished(&mut self, report: &Report, store: &mut UserStore) -> ResultView {
        if let Some(s) = self.running.take().filter(|_| report.converted > 0) {
            store.remember_run(&s);
        }
        let gifs = if self.ctx.summary.piles().videos { self.ctx.summary.animated_gifs } else { 0 };
        result(report, gifs, self.output_folder())
    }

    /// `Urlaub/wolfgang_output`: the output folder alone says too little.
    fn output_folder(&self) -> String {
        let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
        let root = &self.ctx.output_root;
        match (root.parent().and_then(name), name(root)) {
            (Some(parent), Some(folder)) => format!("{parent}/{folder}"),
            (_, folder) => folder.unwrap_or_default(),
        }
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn result(r: &Report, gifs: usize, output_folder: String) -> ResultView {
    let saved = (r.bytes_in > 0 && r.bytes_out > 0).then(|| {
        let pct = 100.0 - r.bytes_out as f64 * 100.0 / r.bytes_in as f64;
        let change = if pct >= 0.5 { format!("{} % kleiner", pct.round()) } else { "nicht kleiner".into() };
        format!("{} → {} ({change})", format_bytes(r.bytes_in), format_bytes(r.bytes_out))
    });
    let mut lines = Vec::new();
    if r.pdf_pictures > 0 {
        lines.push(format!("{} aus PDFs geholt.", plural(r.pdf_pictures, "Bild", "Bilder")));
    }
    if r.transcripts > 0 {
        lines.push(format!("{} aufgeschrieben. Namen und Fachwörter bitte gegenlesen.", plural(r.transcripts, "Aufnahme", "Aufnahmen")));
    }
    if r.tables > 0 {
        lines.push(format!("{} umgewandelt.", plural(r.tables, "Tabelle", "Tabellen")));
    }
    let mut notes: Vec<(Note, usize)> = Vec::new();
    for (_, n) in &r.notes {
        match notes.iter_mut().find(|(x, _)| x == n) {
            Some((_, c)) => *c += 1,
            None => notes.push((*n, 1)),
        }
    }
    for (n, count) in notes {
        lines.push(match n {
            Note::FontUntransformed => format!("{} ohne Glyphen-Umbau gepackt (etwas größer, dafür 1:1).", plural(count, "Schrift", "Schriften")),
            Note::VideoKeptOriginal => format!("{}: Original übernommen, war schon gut gepackt.", plural(count, "Video", "Videos")),
            Note::VideoGrew => format!("{} größer geworden als vorher.", plural(count, "Video ist", "Videos sind")),
            Note::PdfNoPictures => format!("{} ohne Bilder.", plural(count, "PDF", "PDFs")),
            Note::PdfSomeUnreadable => "Einige Bilder in PDFs sind nicht lesbar (z. B. Fax-Scans).".into(),
            Note::PdfAlreadySmall => format!("{} war schon schlank und wurde so übernommen.", plural(count, "PDF", "PDFs")),
        });
    }
    if gifs > 0 && r.converted > 0 {
        lines.push("Die animierten GIFs sind jetzt Videos. Auf Webseiten brauchen sie autoplay, muted, loop und playsinline.".into());
    }
    ResultView {
        converted: r.converted,
        cancelled: r.cancelled,
        saved,
        duration: format_duration(r.duration),
        lines,
        failed: r.failed.iter().map(|(p, e)| (p.display().to_string(), e.clone())).collect(),
        output: (r.converted > 0).then(|| r.output_root.clone()),
        output_folder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use wolfgang_core::image::{self, Pixels};
    use wolfgang_core::settings::ImageSettings;
    use wolfgang_core::ImageFormat;

    fn photo(dir: &std::path::Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        // Each photo its own width, or the second one would be a duplicate.
        let w = 200 + name.len() as u32 * 20;
        let px = Pixels { width: w, height: 200, rgba: vec![90; (w * 200 * 4) as usize], icc: None };
        std::fs::write(dir.join(name), image::encode(&px, ImageFormat::Jpg, 80.0).unwrap()).unwrap();
    }

    #[test]
    fn cards_settings_run_and_remember() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("Bilder");
        photo(&dir, "a.jpg");
        std::fs::write(dir.join("notes.xyz"), "?").unwrap();
        let mut store = UserStore::default();

        let desk = Desk::open(vec![dir.clone()], None, &store).unwrap();
        let v = desk.view();
        assert_eq!(v.piles.iter().map(|p| p.key).collect::<Vec<_>>(), ["images"]);
        assert_eq!((v.fresh, v.known, v.unknown.len()), (1, 0, 1));
        assert_eq!(v.root_name, "Bilder");
        assert_eq!(v.output_folder, "Bilder/wolfgang_output");

        let mut web = WebSettings::default();
        web.naming.prefix = "Kunde Müller".into();
        assert_eq!(desk.preview(&web), "kunde-mueller-0001.webp");

        // A second drop adds to the first.
        let more = t.path().join("Mehr");
        photo(&more, "bb.jpg");
        let mut desk = Desk::open(vec![more], Some(&desk), &store).unwrap();
        assert_eq!(desk.paths.len(), 2);
        assert_eq!(desk.view().piles[0].total, 2);

        let settings = RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Png }), ..Default::default() };
        let plan = desk.plan(settings.clone(), false).unwrap();
        let report = wolfgang_core::execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
        let r = desk.finished(&report, &mut store);
        assert_eq!(r.converted, 2);
        assert!(r.output.is_some() && r.failed.is_empty());
        assert_eq!(store.last_run.as_ref().unwrap().images, settings.images, "remembered for „wie letztes Mal“");

        // Everything known now: only „nochmal“ has something to do.
        let mut desk = Desk::open(desk.paths.clone(), None, &store).unwrap();
        assert_eq!(desk.view().known, 2);
        assert!(desk.plan(settings.clone(), false).is_err());
        assert!(desk.plan(settings, true).is_ok());
    }

    /// Exactly what normal.js's `settings()` sends, so the two can't drift apart.
    #[test]
    fn the_cards_json_is_understood() {
        let web: RunSettings = serde_json::from_str(
            r#"{"images":{"mode":"web","format":"avif","max_side":1200,"quality":70,
                "naming":{"prefix":"Kunde","use_keyword_files":true,"folder_keywords":{"Team":"meeting"},"keywords_all":"buero","separator":"caps","append_original":true}},
               "videos":{"format":"webm","size":null,"quality":"nicer","sound":false,"poster":true,"background":"black"},
               "audio_transcript":{"timestamps":false},
               "pdfs":{"mode":"smaller"}}"#,
        )
        .unwrap();
        let Some(ImageSettings::Web(w)) = &web.images else { panic!("{web:?}") };
        assert_eq!((w.max_side, w.naming.folder_keywords["Team"].as_str()), (1200, "meeting"));
        assert_eq!(web.videos.as_ref().unwrap().size, None, "Original");
        assert!(web.audio_transcript.is_some() && web.validate().is_ok());
        let hq: RunSettings = serde_json::from_str(r#"{"images":{"mode":"high_quality","format":"png"},"audio":{"format":"aac","content":"speech"},"video_transcript":{"timestamps":true}}"#).unwrap();
        assert_eq!(hq.images, Some(ImageSettings::HighQuality { format: ImageFormat::Png }));
        // The web settings alone (name preview) too.
        let _: WebSettings = serde_json::from_value(serde_json::to_value(w).unwrap()).unwrap();
    }

    #[test]
    fn templates_only_set_whats_on_the_desk() {
        let t = tempfile::tempdir().unwrap();
        photo(&t.path().join("x"), "a.jpg");
        let mut desk = Desk::open(vec![t.path().join("x")], None, &UserStore::default()).unwrap();
        let s = RunSettings { images: Some(ImageSettings::Web(WebSettings::default())), videos: Some(Default::default()), ..Default::default() };
        let plan = desk.plan(s, false).unwrap();
        assert!(plan.settings.videos.is_none());
    }
}
