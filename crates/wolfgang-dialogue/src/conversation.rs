//! The state machine. CLI and app only render [`Step`]s and report choices.

use crate::context::{describe_audio, describe_images, describe_transcript, describe_videos, format_bytes, format_duration, format_estimate, join_and, pile_list, Context};
use crate::draft::{Draft, Mode, Template};
use crate::script::{Each, Foreach, InputKind, Line, Mood, Node, Script, Value};
use crate::template::{self, Var};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use wolfgang_core::naming::{self, Separator};
use wolfgang_core::run::{Note, Report};
use wolfgang_core::settings::ImageSettings;
use wolfgang_core::{ImageFormat, RerunPolicy, RunSettings, UserStore};

/// Safety net against `go` loops in a broken dialogue file.
const MAX_HOPS: usize = 64;
const DEFAULT_TEXT_MAX: usize = 80;

/// What the frontend should show next.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    /// Wolfgang says `lines` (one speech bubble each), then waits for an answer.
    Ask {
        node: String,
        mood: Mood,
        lines: Vec<String>,
        answers: Vec<AnswerView>,
        input: Option<InputView>,
        can_go_back: bool,
    },
    /// Start converting with these settings, then call [`Conversation::finished`].
    Start { mood: Mood, lines: Vec<String>, settings: RunSettings, rerun: RerunPolicy },
    /// The user backed out before anything happened.
    Cancelled { mood: Mood, lines: Vec<String> },
    /// Conversation over; call [`Conversation::remember`] to update the user store.
    End { mood: Mood, lines: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AnswerView {
    pub label: String,
    /// Matches the current/default choice (e.g. the user's default format).
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputView {
    pub kind: InputKind,
    pub placeholder: Option<String>,
    pub unit: Option<String>,
    pub min: Option<u32>,
    pub max: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
struct State {
    node: String,
    /// Position inside a `foreach` node.
    item: usize,
    draft: Draft,
}

#[derive(Debug, Clone)]
struct Choice {
    set: Vec<(String, Value)>,
    to: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RunInfo {
    converted: usize,
    failed: usize,
    first_error: String,
    /// Fonts packed the careful way.
    notes: usize,
    kept_original: usize,
    grew: usize,
    pdfs_empty: usize,
    pdfs_unreadable: usize,
    pdf_pictures: usize,
    transcripts: usize,
    bytes_in: u64,
    bytes_out: u64,
    duration: std::time::Duration,
}

pub struct Conversation {
    script: Arc<Script>,
    ctx: Context,
    state: State,
    history: Vec<State>,
    choices: Vec<Choice>,
    current: Step,
    rng: fastrand::Rng,
    last_variant: HashMap<String, usize>,
    run: Option<RunInfo>,
    started_with: Option<RunSettings>,
    /// Answers given up front (CLI flags); questions they answer are skipped.
    fixed: Vec<(String, Value)>,
}

/// Keys that trigger an action instead of storing a value; never re-applied.
const ACTION_KEYS: &[&str] = &["use_last", "preset", "save_preset"];

impl Conversation {
    /// Starts talking. `seed` makes line variants reproducible (tests); use any value otherwise.
    pub fn new(script: Arc<Script>, ctx: Context, seed: u64) -> Self {
        Self::with_fixed(script, ctx, seed, Vec::new())
    }

    /// Like [`Conversation::new`], with some answers already given (e.g. CLI flags as
    /// `("web.max_side", 2000)`). Questions fully answered by them are skipped.
    pub fn with_fixed(script: Arc<Script>, ctx: Context, seed: u64, fixed: Vec<(String, Value)>) -> Self {
        let state = State { node: String::new(), item: 0, draft: Draft::new(&ctx.store.defaults) };
        let start = script.start.clone();
        let mut c = Self {
            script,
            ctx,
            state,
            history: Vec::new(),
            choices: Vec::new(),
            current: Step::End { mood: Mood::Neutral, lines: vec![] },
            rng: fastrand::Rng::with_seed(seed),
            last_variant: HashMap::new(),
            run: None,
            started_with: None,
            fixed,
        };
        c.reapply_fixed();
        c.enter(start, Vec::new());
        c
    }

    pub fn step(&self) -> &Step {
        &self.current
    }

    /// Picks answer number `index` of the current [`Step::Ask`].
    pub fn choose(&mut self, index: usize) -> &Step {
        if let Some(choice) = self.choices.get(index).cloned() {
            self.history.push(self.state.clone());
            for (k, v) in &choice.set {
                self.set(k, v);
            }
            self.advance(choice.to);
        }
        &self.current
    }

    /// Submits the free-text answer. Invalid input keeps the question open
    /// and lets Wolfgang react in character.
    pub fn type_answer(&mut self, text: &str) -> &Step {
        let node = self.node();
        let Some(input) = node.input.clone() else { return &self.current };
        let (min, max) = match input.kind {
            InputKind::Number => (input.min, input.max),
            InputKind::Text => (None, Some(input.max_len.unwrap_or(DEFAULT_TEXT_MAX) as u32)),
        };
        let parsed = match input.kind {
            InputKind::Number => parse_number(text).filter(|n| min.is_none_or(|m| *n >= m) && max.is_none_or(|m| *n <= m)).map(|n| Value::Number(n.into())),
            InputKind::Text => {
                let t = text.trim();
                (!t.is_empty() && t.chars().count() <= max.unwrap_or(u32::MAX) as usize).then(|| Value::String(t.to_owned()))
            }
        };
        match parsed {
            Some(value) => {
                self.history.push(self.state.clone());
                self.set(&input.set, &value);
                self.advance(input.to);
            }
            None => {
                let vars: HashMap<&str, Var> = HashMap::from([
                    ("input", Var::Text(text.trim().to_owned())),
                    ("min", Var::Num(min.unwrap_or(0) as u64)),
                    ("max", Var::Num(max.unwrap_or(0) as u64)),
                ]);
                let mut lines = self.say(&input.invalid, &vars);
                if lines.is_empty() {
                    lines.push("Hm, das versteh ich nicht. Nochmal?".into());
                }
                self.ask(&node, Vec::new(), lines);
            }
        }
        &self.current
    }

    /// Goes back to the previous question.
    pub fn back(&mut self) -> &Step {
        if let Some(prev) = self.history.pop() {
            self.state = prev;
            let node = self.state.node.clone();
            self.enter(node, Vec::new());
            // `enter` re-applied the node's own `set`, which is idempotent.
        }
        &self.current
    }

    /// Reports the finished (or cancelled) run, so Wolfgang can talk about it.
    pub fn finished(&mut self, report: &Report) -> &Step {
        let count = |n: Note| report.notes.iter().filter(|(_, x)| *x == n).count();
        self.run = Some(RunInfo {
            converted: report.converted,
            failed: report.failed.len(),
            first_error: report.failed.first().map(|(p, e)| format!("{}: {e}", p.display())).unwrap_or_default(),
            notes: count(Note::FontUntransformed),
            kept_original: count(Note::VideoKeptOriginal),
            grew: count(Note::VideoGrew),
            pdfs_empty: count(Note::PdfNoPictures),
            pdfs_unreadable: count(Note::PdfSomeUnreadable),
            pdf_pictures: report.pdf_pictures,
            transcripts: report.transcripts,
            bytes_in: report.bytes_in,
            bytes_out: report.bytes_out,
            duration: report.duration,
        });
        self.history.clear();
        let next = if report.cancelled { self.script.after_cancel.clone() } else { self.script.after_run.clone() };
        self.enter(next, Vec::new());
        &self.current
    }

    /// Stores what should be remembered: the settings of a started run as
    /// „letztes Mal“, and a preset if the user named one.
    pub fn remember(&self, store: &mut UserStore) {
        if let Some(settings) = &self.started_with {
            store.remember_run(settings);
            if let Some(name) = &self.state.draft.save_preset {
                let _ = store.save_preset(name, settings.clone());
            }
        }
    }

    /// A line for while the conversion runs.
    pub fn working_line(&mut self) -> String {
        let lines = self.script.working.clone();
        self.pick_line(&lines)
    }

    /// A line for an empty desk (no conversation needed).
    pub fn idle_line(script: &Script, rng: &mut fastrand::Rng) -> String {
        script.idle.get(rng.usize(..script.idle.len().max(1))).cloned().unwrap_or_default()
    }

    pub fn settings(&self) -> RunSettings {
        self.state.draft.settings(self.ctx.piles())
    }

    // ---------------------------------------------------------------------

    fn node(&self) -> Node {
        self.script.nodes.get(&self.state.node).cloned().unwrap_or_default()
    }

    fn advance(&mut self, to: Option<String>) {
        self.advance_with(to, Vec::new());
    }

    fn advance_with(&mut self, to: Option<String>, lines: Vec<String>) {
        let node = self.node();
        if let Some(fe) = node.foreach {
            if self.state.item + 1 < self.items(fe).len() {
                self.state.item += 1;
                let same = self.state.node.clone();
                return self.enter(same, lines);
            }
        }
        self.state.item = 0;
        let target = to.or(node.then).unwrap_or_else(|| "@end".into());
        self.enter(target, lines);
    }

    fn enter(&mut self, mut target: String, mut lines: Vec<String>) {
        let mut mood = Mood::Neutral;
        for _ in 0..MAX_HOPS {
            if target.starts_with('@') {
                self.current = self.terminal(&target, mood, lines);
                return;
            }
            let Some(node) = self.script.nodes.get(&target).cloned() else { break };
            self.state.node = target.clone();
            if let Some(fe) = node.foreach {
                if self.state.item >= self.items(fe).len() {
                    self.state.item = 0;
                    target = node.then.clone().unwrap_or_else(|| "@end".into());
                    continue;
                }
            }
            for (k, v) in &node.set {
                self.set(k, v);
            }
            let item_vars = self.item_vars(&node);
            let own = self.say(&node.say, &item_vars);
            mood = node.mood;
            if node.go.is_empty() {
                return self.ask(&node, lines, own);
            }
            lines.extend(own);
            match node.go.iter().find(|g| self.all(&g.when)) {
                Some(g) => target = g.to.clone(),
                None => break,
            }
        }
        // Only reachable with a broken dialogue file.
        lines.push("Huch, da hab ich mich verheddert. Fang lieber nochmal von vorn an.".into());
        self.current = Step::End { mood: Mood::Sorry, lines };
    }

    /// Shows the question: `before` are lines from earlier nodes, `own` the node's own.
    fn ask(&mut self, node: &Node, before: Vec<String>, own: Vec<String>) {
        let item_vars = self.item_vars(node);
        let mut answers = Vec::new();
        let mut choices = Vec::new();
        for a in node.answers.iter().filter(|a| self.all(&a.when)) {
            let items: Vec<Option<String>> = match a.each {
                Some(Each::Presets) => self.ctx.store.presets.iter().map(|p| Some(p.name.clone())).collect(),
                None => vec![None],
            };
            for item in items {
                let mut vars = item_vars.clone();
                if let Some(name) = &item {
                    vars.insert("preset", Var::Text(name.clone()));
                }
                let set: Vec<(String, Value)> = a.set.iter().map(|(k, v)| (k.clone(), self.render_value(v, &vars))).collect();
                let current = a.each.is_none() && !set.is_empty() && set.iter().all(|(k, v)| self.state.draft.get(k).as_ref() == Some(v));
                answers.push(AnswerView { label: self.render(&a.label, &vars), current });
                choices.push(Choice { set, to: a.to.clone() });
            }
        }

        // Already answered up front? Skip the question, keep what was said before it.
        if !self.fixed.is_empty() {
            let is_fixed = |k: &str, v: &Value| self.fixed.iter().any(|(fk, fv)| fk == k && fv == v);
            if let Some(c) = choices.iter().find(|c| !c.set.is_empty() && c.set.iter().all(|(k, v)| is_fixed(k, v))) {
                let c = c.clone();
                for (k, v) in &c.set {
                    self.set(k, v);
                }
                return self.advance_with(c.to, before);
            }
            if let Some(inp) = node.input.as_ref().filter(|i| self.fixed.iter().any(|(k, _)| *k == i.set)) {
                return self.advance_with(inp.to.clone(), before);
            }
        }

        self.choices = choices;
        let mut lines = before;
        lines.extend(own);
        let input = node.input.as_ref().map(|i| InputView {
            kind: i.kind,
            placeholder: i.placeholder.as_ref().map(|p| self.render(p, &item_vars)),
            unit: i.unit.clone(),
            min: i.min,
            max: if i.kind == InputKind::Text { Some(i.max_len.unwrap_or(DEFAULT_TEXT_MAX) as u32) } else { i.max },
        });
        self.current = Step::Ask { node: self.state.node.clone(), mood: node.mood, lines, answers, input, can_go_back: !self.history.is_empty() };
    }

    fn terminal(&mut self, target: &str, mood: Mood, lines: Vec<String>) -> Step {
        self.history.clear();
        match target {
            "@start" => {
                let settings = self.settings();
                self.started_with = Some(settings.clone());
                Step::Start { mood, lines, settings, rerun: self.state.draft.rerun }
            }
            "@cancel" => Step::Cancelled { mood, lines },
            _ => Step::End { mood, lines },
        }
    }

    fn set(&mut self, key: &str, value: &Value) {
        let folder = self.current_folder();
        if let Err(e) = self.state.draft.apply(key, value, &self.ctx.store, folder.as_deref()) {
            debug_assert!(false, "dialogue set {key}: {e}");
        }
        self.reapply_fixed(); // flags win over last run, presets and answers
    }

    fn reapply_fixed(&mut self) {
        let fixed = std::mem::take(&mut self.fixed);
        for (k, v) in fixed.iter().filter(|(k, _)| !ACTION_KEYS.contains(&k.as_str())) {
            let _ = self.state.draft.apply(k, v, &self.ctx.store, None);
        }
        self.fixed = fixed;
    }

    fn items(&self, fe: Foreach) -> Vec<String> {
        match fe {
            Foreach::FoldersWithoutKeywords => self.ctx.summary.folders_without_keywords.clone(),
        }
    }

    fn current_folder(&self) -> Option<String> {
        let fe = self.script.nodes.get(&self.state.node)?.foreach?;
        self.items(fe).get(self.state.item).cloned()
    }

    fn item_vars(&self, node: &Node) -> HashMap<&'static str, Var> {
        let mut vars = HashMap::new();
        if node.foreach.is_some() {
            if let Some(folder) = self.current_folder() {
                vars.insert("folder", Var::Text(if folder.is_empty() { "Hauptordner".into() } else { folder }));
            }
        }
        vars
    }

    fn say(&mut self, lines: &[Line], extra: &HashMap<&str, Var>) -> Vec<String> {
        let mut out = Vec::new();
        for line in lines {
            if !self.all(line.condition()) {
                continue;
            }
            let variants: Vec<String> = line.variants().into_iter().map(str::to_owned).collect();
            let text = self.pick_line(&variants);
            out.push(self.render(&text, extra));
        }
        out
    }

    /// Random variant, but never the same one twice in a row.
    fn pick_line(&mut self, variants: &[String]) -> String {
        if variants.is_empty() {
            return String::new();
        }
        let key = variants[0].clone();
        let mut i = self.rng.usize(..variants.len());
        if variants.len() > 1 && self.last_variant.get(&key) == Some(&i) {
            i = (i + 1) % variants.len();
        }
        self.last_variant.insert(key, i);
        let text = variants[i].clone();
        self.render(&text, &HashMap::new())
    }

    fn render(&self, text: &str, extra: &HashMap<&str, Var>) -> String {
        template::render(text, |name| extra.get(name).cloned().or_else(|| self.var(name)))
    }

    fn render_value(&self, v: &Value, extra: &HashMap<&str, Var>) -> Value {
        match v {
            Value::String(s) => Value::String(self.render(s, extra)),
            other => other.clone(),
        }
    }

    fn all(&self, when: &[String]) -> bool {
        when.iter().all(|w| match w.strip_prefix('!') {
            Some(f) => !self.flag(f),
            None => self.flag(w),
        })
    }

    fn flag(&self, name: &str) -> bool {
        let s = &self.ctx.summary;
        let d = &self.state.draft;
        let run = self.run.as_ref();
        let p = self.ctx.piles();
        let piles = s.convertible();
        match name {
            "has_images" => p.images,
            "has_videos" => p.videos,
            "has_audio" => p.audio,
            "has_fonts" => p.fonts,
            "has_pdfs" => p.pdfs,
            "has_gifs" => p.videos && s.animated_gifs > 0,
            "transparent_gifs" => p.videos && s.transparent_gifs > 0,
            "nothing_to_ask" => !p.media() && (p.fonts || p.pdfs),
            "takes_a_while" => self.estimate().is_some_and(|d| d.as_secs() >= 60),
            "videos_kept_original" => run.is_some_and(|r| r.kept_original > 0),
            "videos_grew" => run.is_some_and(|r| r.grew > 0),
            "pdfs_without_pictures" => run.is_some_and(|r| r.pdfs_empty > 0),
            "pdfs_unreadable" => run.is_some_and(|r| r.pdfs_unreadable > 0),
            "pictures_from_pdfs" => run.is_some_and(|r| r.pdf_pictures > 0),
            "saved_bytes" => run.is_some_and(|r| r.bytes_in > 0),
            "has_media" => p.media(),
            "only_images" => p.images && !p.videos && !p.audio && !p.fonts,
            "several_piles" => piles.len() > 1,
            "videos_have_sound" => s.videos_with_sound > 0,
            "media_tools_missing" => s.media_tools_missing,
            "images_asked" => d.images_asked,
            "videos_asked" => d.videos_asked,
            "audio_asked" => d.audio_asked,
            "transcription_installed" => self.ctx.transcription,
            "transcript_asked" => d.transcript_asked,
            "transcript_explained" => d.transcript_explained,
            "wrote_transcripts" => run.is_some_and(|r| r.transcripts > 0),
            "has_unknown" => !s.unknown.is_empty(),
            "has_duplicates" => s.duplicates > 0,
            "nothing_to_do" => piles.is_empty(),
            "only_fonts" => p.fonts && !p.media(),
            "has_last_run" => self.ctx.last_run().is_some(),
            "has_presets" => p.media() && !self.ctx.store.presets.is_empty(),
            "has_known" => piles.iter().any(|p| p.known > 0),
            "has_new" => piles.iter().any(|p| p.fresh() > 0),
            "keyword_files_found" => !s.keyword_files.is_empty(),
            "folders_without_keywords" => !s.folders_without_keywords.is_empty(),
            "several_folders_without_keywords" => s.folders_without_keywords.len() > 1,
            "mode_web" => d.mode == Some(Mode::Web),
            "mode_hq" => d.mode == Some(Mode::Hq),
            "from_template" => d.template.is_some(),
            "rerun_all" => d.rerun == RerunPolicy::All,
            "failed_any" => run.is_some_and(|r| r.failed > 0),
            "all_failed" => run.is_some_and(|r| r.failed > 0 && r.converted == 0),
            "converted_any" => run.is_some_and(|r| r.converted > 0),
            "has_notes" => run.is_some_and(|r| r.notes > 0),
            "saved_preset" => d.save_preset.is_some(),
            _ => false,
        }
    }

    fn var(&self, name: &str) -> Option<Var> {
        let s = &self.ctx.summary;
        let run = self.run.clone().unwrap_or_default();
        let num = |n: usize| Some(Var::Num(n as u64));
        let text = |t: String| Some(Var::Text(t));
        let piles = s.convertible();
        match name {
            "images" => num(s.images.total),
            "videos" => num(s.videos.total),
            "audio_files" => num(s.audio.total),
            "pdfs" => num(s.pdfs.total),
            "animated_gifs" => num(s.animated_gifs),
            "videos_kept_original" => num(run.kept_original),
            "videos_grew" => num(run.grew),
            "pdfs_without_pictures" => num(run.pdfs_empty),
            "pdf_pictures" => num(run.pdf_pictures),
            "transcripts" => num(run.transcripts),
            "estimate" => text(self.estimate().map(format_estimate).unwrap_or_default()),
            "fonts" => num(s.fonts.total),
            "unknown" => num(s.unknown.len()),
            "duplicates" => num(s.duplicates),
            "new_images" => num(s.images.fresh()),
            "known_images" => num(s.images.known),
            "new_fonts" => num(s.fonts.fresh()),
            "known_fonts" => num(s.fonts.known),
            "new_files" => num(piles.iter().map(|p| p.fresh()).sum()),
            "known_files" => num(piles.iter().map(|p| p.known).sum()),
            "keyword_folders" => num(s.keyword_files.len()),
            "folders_without_keywords" => num(s.folders_without_keywords.len()),
            "converted" => num(run.converted),
            "failed" => num(run.failed),
            "saved_percent" => num((run.bytes_out * 100).checked_div(run.bytes_in).map_or(0, |kept| 100usize.saturating_sub(kept as usize))),
            "last_run" => text(self.ctx.describe_last_run()),
            "pile_list" => text(pile_list(s)),
            "summary" => text(self.summary_text()),
            "output_folder" => text(self.output_folder()),
            "default_web_format" => text(ImageFormat::from(self.ctx.store.defaults.web_format).label().into()),
            "preview_dash" => text(self.preview(Separator::Dash)),
            "preview_underscore" => text(self.preview(Separator::Underscore)),
            "preview_caps" => text(self.preview(Separator::Caps)),
            "bytes_in" => text(format_bytes(run.bytes_in)),
            "bytes_out" => text(format_bytes(run.bytes_out)),
            "duration" => text(format_duration(run.duration)),
            "first_error" => text(run.first_error),
            "save_preset_name" => text(self.state.draft.save_preset.clone().unwrap_or_default()),
            _ => None,
        }
    }

    /// How long the videos and transcripts will take, from what ffprobe found (`None`
    /// without either, or if a file couldn't be read). Other piles are quick in comparison.
    fn estimate(&self) -> Option<std::time::Duration> {
        let settings = self.settings();
        let only_new = self.state.draft.rerun == RerunPolicy::OnlyNew;
        let s = &self.ctx.summary;
        let infos = |facts: &'_ [wolfgang_core::workspace::VideoFacts]| -> Option<Vec<wolfgang_core::media::MediaInfo>> {
            facts.iter().filter(|f| !(only_new && f.known)).map(|f| f.info.clone()).collect()
        };
        let mut total = None;
        if let Some(v) = &settings.videos {
            let videos = infos(&s.video_facts)?;
            total = Some(wolfgang_core::media::estimate(&videos.iter().collect::<Vec<_>>(), v)?);
        }
        let mut lengths = Vec::new();
        for (on, facts) in [(settings.video_transcript.is_some(), &s.video_facts), (settings.audio_transcript.is_some(), &s.audio_facts)] {
            if on {
                lengths.extend(infos(facts)?.iter().map(|i| i.duration).collect::<Option<Vec<f64>>>()?);
            }
        }
        if !lengths.is_empty() {
            total = Some(total.unwrap_or_default() + wolfgang_core::transcribe::estimate(&lengths));
        }
        total
    }

    /// `Urlaub/wolfgang_output`: the output folder alone says too little.
    fn output_folder(&self) -> String {
        let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
        let folder = name(&self.ctx.output_root).unwrap_or_default();
        match self.ctx.output_root.parent().and_then(name) {
            Some(parent) => format!("{parent}/{folder}"),
            None => folder,
        }
    }

    /// Example file name for the first image with the current naming choices.
    fn preview(&self, sep: Separator) -> String {
        let w = &self.state.draft.web;
        let n = &w.naming;
        let (dir, stem) = self.ctx.sample_image.clone().unwrap_or_else(|| (String::new(), "bild".into()));
        let keywords = match self.ctx.keyword_words.get(&dir).filter(|_| n.use_keyword_files) {
            Some(words) => words.clone(),
            None => naming::keyword_words(n.folder_keywords.get(&dir).filter(|k| !k.is_empty()).unwrap_or(&n.keywords_all)),
        };
        let original = n.append_original.then_some(stem.as_str());
        let ext = ImageFormat::from(w.format).ext();
        format!("{}.{ext}", naming::seo_name(&n.prefix, &keywords, original, &stem, 1, sep))
    }

    /// „Also: 42 Fotos fürs Web als WebP, 1600 px, Qualität 80 …“
    fn summary_text(&self) -> String {
        let s = &self.ctx.summary;
        let d = &self.state.draft;
        let only_new = d.rerun == RerunPolicy::OnlyNew;
        let count = |pile: &wolfgang_core::workspace::PileSummary, one: &str, many: &str| {
            let n = if only_new { pile.fresh() } else { pile.total };
            if n == 1 { one.to_owned() } else { format!("{n} {many}") }
        };
        let what = |pile: &wolfgang_core::workspace::PileSummary, one: &str, many: &str, new_one: &str, new_many: &str| {
            if only_new && pile.known > 0 {
                count(pile, new_one, new_many)
            } else {
                count(pile, one, many)
            }
        };
        let settings = self.settings();
        let mut parts = Vec::new();
        if let Some(images) = &settings.images {
            let mut t = format!("{} {}", what(&s.images, "ein Foto", "Fotos", "das neue Foto", "neue Fotos"), describe_images(images));
            if let ImageSettings::Web(w) = images {
                t.push_str(&format!(" – Dateinamen wie „{}“", self.preview(w.naming.separator)));
            }
            parts.push(t);
        }
        if let Some(v) = &settings.videos {
            // Without any sound track, „mit Ton“ would be a promise Wolfgang can't keep.
            let v = wolfgang_core::VideoSettings { sound: v.sound && s.videos_with_sound > 0, ..v.clone() };
            let mut t = format!("{} {}", what(&s.videos, "ein Video", "Videos", "das neue Video", "neue Videos"), describe_videos(&v));
            if s.transparent_gifs > 0 {
                t.push_str(&format!(", Durchsichtiges auf {}", v.background.label()));
            }
            parts.push(t);
        }
        if let Some(t) = &settings.video_transcript {
            parts.push(format!("{} {}", what(&s.videos, "ein Video", "Videos", "das neue Video", "neue Videos"), describe_transcript(t)));
        }
        if let Some(a) = &settings.audio {
            parts.push(format!("{} {}", what(&s.audio, "eine Audiodatei", "Audiodateien", "die neue Audiodatei", "neue Audiodateien"), describe_audio(a)));
        }
        if let Some(t) = &settings.audio_transcript {
            parts.push(format!("{} {}", what(&s.audio, "eine Audiodatei", "Audiodateien", "die neue Audiodatei", "neue Audiodateien"), describe_transcript(t)));
        }
        let mut text = if parts.len() > 2 { parts.join("; ") } else { join_and(&parts) };
        // Piles without questions come last: „…, dazu die Bilder aus 2 PDFs und eine Schrift als WOFF2“.
        let mut extras = Vec::new();
        if settings.pdfs.is_some() {
            extras.push(if (if only_new { s.pdfs.fresh() } else { s.pdfs.total }) == 1 {
                "die Bilder aus dem PDF".to_owned()
            } else {
                format!("die Bilder aus {}", count(&s.pdfs, "einem PDF", "PDFs"))
            });
        }
        if self.ctx.has_fonts() {
            extras.push(format!("{} als WOFF2", count(&s.fonts, "eine Schrift", "Schriften")));
        }
        if !extras.is_empty() {
            let extras = join_and(&extras);
            text = if text.is_empty() { extras } else { format!("{text}, dazu {extras}") };
        }
        let mut text = format!("Also: {text}.");
        if d.template == Some(Template::LastRun) {
            text = format!("{} So wie letztes Mal.", text);
        }
        text.push_str(&format!(" Landet alles in „{}“.", self.output_folder()));
        text
    }
}

/// Accepts `2000`, `2.000`, `2000px`, `2000 Pixel`, `80 %`.
fn parse_number(text: &str) -> Option<u32> {
    let t = text.trim().to_lowercase();
    let t = t.trim_end_matches("pixel").trim_end_matches("px").trim_end_matches('%').trim();
    let digits: String = t.chars().filter(|c| *c != '.' && *c != ' ').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::parse_number;

    #[test]
    fn parses_friendly_numbers() {
        assert_eq!(parse_number(" 2000 "), Some(2000));
        assert_eq!(parse_number("2.000"), Some(2000));
        assert_eq!(parse_number("1400px"), Some(1400));
        assert_eq!(parse_number("1400 Pixel"), Some(1400));
        assert_eq!(parse_number("80 %"), Some(80));
        assert_eq!(parse_number("zwei"), None);
        assert_eq!(parse_number("-5"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("99999999999"), None);
    }
}
