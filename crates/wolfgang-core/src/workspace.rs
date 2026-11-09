//! A drop on Wolfgang's desk: what's there, where the results go, what's new,
//! and — once settings are known — the concrete list of jobs.

use crate::error::{io_err, Result};
use crate::font;
use crate::gif;
use crate::image::{ColorTarget, ImageTask};
use crate::kind::FileKind;
use crate::media::{self, MediaInfo, VideoTask};
use crate::naming;
use crate::runlog::{self, RunLog};
use crate::scan::{self, Scan, ScannedFile, OUTPUT_DIR};
use crate::settings::{self, AudioSettings, ImageSettings, Naming, RunSettings};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Output folder for font results, inside the output root.
pub const FONTS_DIR: &str = "fonts";
/// Poster frames of videos are WebP, named like the video.
pub const POSTER_EXT: &str = "webp";

pub struct Workspace {
    pub scan: Scan,
    /// Where results go, e.g. `~/Bilder/Urlaub/wolfgang_output`.
    pub output_root: PathBuf,
    /// Identifies the drop so a later drop of the same thing finds this folder.
    pub source_key: String,
    pub log: RunLog,
    /// Content hash per entry of `scan.files` (unknown files get an empty hash).
    hashes: Vec<String>,
    /// Per entry of `scan.files`: what ffprobe and the GIF reader found out.
    facts: Vec<Facts>,
}

/// What was found out about a file while opening the drop.
#[derive(Debug, Clone, Default)]
struct Facts {
    /// Videos only; `None` if ffprobe couldn't read it (dealt with when converting).
    media: Option<MediaInfo>,
    /// An animated GIF (now in the video pile).
    animated_gif: bool,
    /// … with transparent areas.
    transparent: bool,
}

/// A video on the desk, for time estimates.
#[derive(Debug, Clone, Default)]
pub struct VideoFacts {
    /// `None` if ffprobe couldn't read it.
    pub info: Option<MediaInfo>,
    /// Converted in an earlier run.
    pub known: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PileSummary {
    pub total: usize,
    /// Already converted in an earlier run into the same output folder.
    pub known: usize,
    pub bytes: u64,
}

impl PileSummary {
    pub fn fresh(&self) -> usize {
        self.total - self.known
    }
}

/// Which piles can be worked on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Piles {
    pub images: bool,
    pub videos: bool,
    pub audio: bool,
    pub pdfs: bool,
    pub fonts: bool,
}

impl Piles {
    pub const ALL: Piles = Piles { images: true, videos: true, audio: true, pdfs: true, fonts: true };

    /// Images, videos or audio: everything that has questions.
    pub fn media(self) -> bool {
        self.images || self.videos || self.audio
    }
}

impl Summary {
    /// Videos and audio only count if the tools to convert them are there.
    pub fn piles(&self) -> Piles {
        let tools = !self.media_tools_missing;
        Piles {
            images: self.images.total > 0,
            videos: tools && self.videos.total > 0,
            audio: tools && self.audio.total > 0,
            pdfs: self.pdfs.total > 0,
            fonts: self.fonts.total > 0,
        }
    }

    /// All piles that get converted, for totals.
    pub fn convertible(&self) -> Vec<&PileSummary> {
        let p = self.piles();
        [(p.images, &self.images), (p.videos, &self.videos), (p.audio, &self.audio), (p.pdfs, &self.pdfs), (p.fonts, &self.fonts)]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, s)| s)
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub images: PileSummary,
    pub videos: PileSummary,
    pub audio: PileSummary,
    pub pdfs: PileSummary,
    pub fonts: PileSummary,
    /// Videos with a sound track (or that couldn't be checked).
    pub videos_with_sound: usize,
    /// Animated GIFs; they count as videos.
    pub animated_gifs: usize,
    /// Animated GIFs with transparent areas (Wolfgang asks for a background).
    pub transparent_gifs: usize,
    /// Every video, for estimating how long they take.
    pub video_facts: Vec<VideoFacts>,
    /// There are videos or audio, but the bundled ffmpeg can't be found.
    pub media_tools_missing: bool,
    pub unknown: Vec<PathBuf>,
    /// Files with identical content to an earlier file in this drop.
    pub duplicates: usize,
    /// Number of earlier runs into this output folder.
    pub previous_runs: usize,
    /// Image folders with a keyword file: (relative folder, file path).
    pub keyword_files: Vec<(String, PathBuf)>,
    /// Image folders without a keyword file.
    pub folders_without_keywords: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerunPolicy {
    /// Skip files converted in an earlier run.
    #[default]
    OnlyNew,
    /// Convert everything again (known files keep their index).
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Task {
    Image(ImageTask),
    Font,
    Video(VideoTask),
    Audio(AudioSettings),
    /// `target` is the folder the pictures go into.
    Pdf,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub source: PathBuf,
    pub rel: PathBuf,
    pub hash: String,
    pub size: u64,
    /// Output path relative to the output root.
    pub target: PathBuf,
    pub index: Option<u32>,
    /// Relative source folder (index counter scope).
    pub dir: String,
    pub task: Task,
}

impl Job {
    /// Every file this job writes, relative to the output root.
    pub fn outputs(&self) -> Vec<&Path> {
        let mut v = vec![self.target.as_path()];
        if let Task::Video(VideoTask { poster: Some(p), .. }) = &self.task {
            v.push(p);
        }
        v
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub output_root: PathBuf,
    pub source_key: String,
    pub settings: RunSettings,
    pub jobs: Vec<Job>,
    pub skipped_known: Vec<PathBuf>,
    pub skipped_duplicates: Vec<PathBuf>,
    /// Files that can't be handled, with the reason (e.g. a broken font).
    pub problems: Vec<(PathBuf, String)>,
}

impl Workspace {
    /// Scans the dropped items, finds (or picks) the output folder and hashes all files.
    pub fn open(items: &[PathBuf]) -> Result<Self> {
        let mut scan = scan::scan(items)?;
        // Animated GIFs are videos; still ones stay photos.
        let gifs: Vec<Option<gif::GifInfo>> = scan
            .files
            .par_iter()
            .map(|f| {
                let is_gif = f.kind == FileKind::Image && f.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gif"));
                is_gif.then(|| std::fs::read(&f.path).ok().and_then(|d| gif::info(&d))).flatten()
            })
            .collect();
        for (f, g) in scan.files.iter_mut().zip(&gifs) {
            if g.is_some_and(|g| g.animated()) {
                f.kind = FileKind::Video;
            }
        }
        let (output_root, source_key, log) = locate_output(&scan.items)?;
        let hashes = scan
            .files
            .par_iter()
            .map(|f| if f.kind == FileKind::Unknown { Ok(String::new()) } else { hash_file(&f.path) })
            .collect::<Result<Vec<_>>>()?;
        // For the questions (sound?) and time estimates; a broken file is dealt with later.
        let facts = scan
            .files
            .par_iter()
            .zip(&gifs)
            .map(|(f, g)| Facts {
                media: (f.kind == FileKind::Video && media::tools().is_some()).then(|| media::probe(&f.path).ok()).flatten(),
                animated_gif: f.kind == FileKind::Video && g.is_some(),
                transparent: f.kind == FileKind::Video && g.is_some_and(|g| g.transparent),
            })
            .collect();
        Ok(Self { scan, output_root, source_key, log, hashes, facts })
    }

    /// Leaves a whole pile alone (e.g. `--only fonts`): its files are neither
    /// counted nor converted.
    pub fn exclude(&mut self, kind: FileKind) {
        self.retain(|k| k != kind);
    }

    /// Leaves every pile but this one alone (`--only videos`). Unknown files are still listed.
    pub fn keep_only(&mut self, kind: FileKind) {
        self.retain(|k| k == kind || k == FileKind::Unknown);
    }

    fn retain(&mut self, keep: impl Fn(FileKind) -> bool) {
        let keep: Vec<bool> = self.scan.files.iter().map(|f| keep(f.kind)).collect();
        let mut k = keep.iter();
        self.scan.files.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        self.hashes.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        self.facts.retain(|_| *k.next().unwrap());
        if self.scan.count(FileKind::Image) == 0 {
            self.scan.keyword_files.clear();
        }
    }

    /// Known kinds with their hash and what was found out about them.
    fn convertible(&self) -> impl Iterator<Item = (&ScannedFile, &String, &Facts)> {
        self.scan.files.iter().zip(&self.hashes).zip(&self.facts).filter(|((f, _), _)| f.kind != FileKind::Unknown).map(|((f, h), x)| (f, h, x))
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary { previous_runs: self.log.runs, ..Default::default() };
        let mut seen = HashSet::new();
        for (f, hash, facts) in self.convertible() {
            if !seen.insert(hash) {
                s.duplicates += 1;
                continue;
            }
            let pile = match f.kind {
                FileKind::Image => &mut s.images,
                FileKind::Video => {
                    // Unreadable → unknown sound → better ask.
                    s.videos_with_sound += usize::from(facts.media.as_ref().is_none_or(|m| m.audio.is_some()));
                    s.animated_gifs += usize::from(facts.animated_gif);
                    s.transparent_gifs += usize::from(facts.transparent);
                    s.video_facts.push(VideoFacts { info: facts.media.clone(), known: self.log.files.contains_key(hash) });
                    &mut s.videos
                }
                FileKind::Audio => &mut s.audio,
                FileKind::Pdf => &mut s.pdfs,
                _ => &mut s.fonts,
            };
            pile.total += 1;
            pile.bytes += f.size;
            if self.log.files.contains_key(hash) {
                pile.known += 1;
            }
        }
        s.unknown = self.scan.of_kind(FileKind::Unknown).map(|f| f.rel.clone()).collect();
        s.media_tools_missing = s.videos.total + s.audio.total > 0 && media::tools().is_none();
        for dir in self.scan.image_dirs() {
            match self.scan.keyword_files.get(&dir) {
                Some(k) => s.keyword_files.push((dir, k.path.clone())),
                None => s.folders_without_keywords.push(dir),
            }
        }
        s
    }

    /// Turns settings into concrete jobs with final file names.
    pub fn plan(&self, settings: &RunSettings, policy: RerunPolicy) -> Result<Plan> {
        settings.validate()?;
        let mut plan = Plan {
            output_root: self.output_root.clone(),
            source_key: self.source_key.clone(),
            settings: settings.clone(),
            jobs: Vec::new(),
            skipped_known: Vec::new(),
            skipped_duplicates: Vec::new(),
            problems: Vec::new(),
        };
        let mut next_index: HashMap<String, u32> = HashMap::new();
        let mut taken: HashSet<String> = HashSet::new(); // lowercase: macOS file systems ignore case
        let mut seen = HashSet::new();

        for (f, hash, _) in self.convertible() {
            let wanted = match f.kind {
                FileKind::Image => settings.images.is_some(),
                FileKind::Font => settings.fonts.is_some(),
                FileKind::Video => settings.videos.is_some(),
                FileKind::Audio => settings.audio.is_some(),
                FileKind::Pdf => settings.pdfs.is_some(),
                _ => false,
            };
            if !wanted {
                continue;
            }
            if !seen.insert(hash.clone()) {
                plan.skipped_duplicates.push(f.rel.clone());
                continue;
            }
            let known = self.log.files.get(hash);
            if known.is_some() && policy == RerunPolicy::OnlyNew {
                plan.skipped_known.push(f.rel.clone());
                continue;
            }
            if matches!(f.kind, FileKind::Video | FileKind::Audio) && media::tools().is_none() {
                plan.problems.push((f.rel.clone(), "Mein Werkzeug für Videos und Audio (ffmpeg) fehlt".into()));
                continue;
            }
            let previous_outputs: HashSet<&str> = known.map(|k| k.outputs.iter().map(String::as_str).collect()).unwrap_or_default();
            let dir = f.rel_dir();
            let mut job = Job { source: f.path.clone(), rel: f.rel.clone(), hash: hash.clone(), size: f.size, target: PathBuf::new(), index: None, dir, task: Task::Font };

            let stem = match (f.kind, &settings.images) {
                (FileKind::Image, Some(ImageSettings::HighQuality { format })) => {
                    job.task = Task::Image(ImageTask { format: *format, max_side: None, quality: settings::high_quality(*format), color: ColorTarget::KeepSource });
                    file_stem(&f.rel)
                }
                (FileKind::Image, Some(ImageSettings::Web(w))) => {
                    let format = w.format.into();
                    job.task = Task::Image(ImageTask { format, max_side: Some(w.max_side), quality: settings::encoder_quality(format, w.quality), color: ColorTarget::Srgb });
                    let index = known.and_then(|k| k.index).unwrap_or_else(|| {
                        let n = next_index.entry(job.dir.clone()).or_insert_with(|| self.log.max_index.get(&job.dir).copied().unwrap_or(0));
                        *n += 1;
                        *n
                    });
                    job.index = Some(index);
                    let keywords = self.keywords_for(&job.dir, &w.naming);
                    let original = w.naming.append_original.then(|| file_stem(&f.rel));
                    naming::seo_name(&w.naming.prefix, &keywords, original.as_deref(), &file_stem(&f.rel), index, w.naming.separator)
                }
                (FileKind::Video, _) => {
                    let v = settings.videos.clone().expect("wanted");
                    job.task = Task::Video(VideoTask { poster: None, settings: v });
                    web_safe_stem(&f.rel, "video")
                }
                (FileKind::Audio, _) => {
                    job.task = Task::Audio(settings.audio.expect("wanted"));
                    web_safe_stem(&f.rel, "audio")
                }
                (FileKind::Pdf, _) => {
                    job.task = Task::Pdf;
                    web_safe_stem(&f.rel, "pdf")
                }
                (FileKind::Font, _) => match std::fs::read(&f.path).map_err(io_err(&f.path)).and_then(|d| font::to_sfnt(&d)) {
                    Ok(sfnt) => font::web_name(&sfnt).unwrap_or_else(|| naming::join(&naming::words(&file_stem(&f.rel)), naming::Separator::Dash)),
                    Err(e) => {
                        plan.problems.push((f.rel.clone(), e.to_string()));
                        continue;
                    }
                },
                _ => unreachable!("filtered above"),
            };

            let (folder, ext) = match &job.task {
                Task::Image(t) => (PathBuf::from(&job.dir), t.format.ext()),
                Task::Font => (Path::new(FONTS_DIR).join(&job.dir), "woff2"),
                Task::Video(t) => (PathBuf::from(&job.dir), t.settings.format.ext()),
                Task::Audio(a) => (PathBuf::from(&job.dir), a.format.ext()),
                Task::Pdf => (PathBuf::from(&job.dir), ""), // a folder
            };
            let poster = matches!(&job.task, Task::Video(t) if t.settings.poster);
            let exts: &[&str] = if poster { &[ext, POSTER_EXT] } else { &[ext] };
            let original_ext = f.rel.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let name = self.free_name(&folder, &stem, &original_ext, exts, &mut taken, &previous_outputs);
            job.target = folder.join(with_ext(&name, ext));
            if let Task::Video(t) = &mut job.task {
                t.poster = poster.then(|| folder.join(format!("{name}.{POSTER_EXT}")));
            }
            plan.jobs.push(job);
        }
        Ok(plan)
    }

    /// Keyword file > typed keywords for this folder > typed keywords for all.
    fn keywords_for(&self, dir: &str, n: &Naming) -> Vec<String> {
        if n.use_keyword_files {
            if let Some(k) = self.scan.keyword_files.get(dir) {
                return k.words.clone();
            }
        }
        let typed = n.folder_keywords.get(dir).filter(|t| !t.trim().is_empty()).unwrap_or(&n.keywords_all);
        naming::keyword_words(typed)
    }

    /// `stem`, or `stem-<origext>` / `stem-2` … if `stem.<ext>` is taken (for any
    /// of `exts`) in this run or by a file on disk that isn't this source's own earlier output.
    fn free_name(&self, folder: &Path, stem: &str, original_ext: &str, exts: &[&str], taken: &mut HashSet<String>, own: &HashSet<&str>) -> String {
        let candidates = std::iter::once(stem.to_owned())
            .chain((!original_ext.is_empty()).then(|| format!("{stem}-{original_ext}")))
            .chain((2..).map(|n| format!("{stem}-{n}")));
        for name in candidates {
            let rels: Vec<String> = exts.iter().map(|ext| rel_string(&folder.join(with_ext(&name, ext)))).collect();
            let free = rels.iter().all(|rel| {
                // A folder (no extension) is this source's own if its earlier outputs are in it.
                let ours = own.contains(rel.as_str()) || own.iter().any(|o| o.starts_with(&format!("{rel}/")));
                let on_disk = self.output_root.join(rel).exists() && !ours;
                !on_disk && !taken.contains(&rel.to_lowercase())
            });
            if free {
                taken.extend(rels.iter().map(|r| r.to_lowercase()));
                return name;
            }
        }
        unreachable!("infinite candidates")
    }
}

pub(crate) fn rel_string(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// `name.ext`, or just `name` for folders.
fn with_ext(name: &str, ext: &str) -> String {
    if ext.is_empty() {
        name.to_owned()
    } else {
        format!("{name}.{ext}")
    }
}

/// „Drohne Strand FINAL.MOV“ → `drohne-strand-final`
fn web_safe_stem(p: &Path, fallback: &str) -> String {
    let name = naming::join(&naming::words(&file_stem(p)), naming::Separator::Dash);
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name
    }
}

fn file_stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn hash_file(path: &Path) -> Result<String> {
    let mut h = blake3::Hasher::new();
    let f = std::fs::File::open(path).map_err(io_err(path))?;
    h.update_reader(f).map_err(io_err(path))?;
    Ok(h.finalize().to_hex().to_string())
}

/// Results go into `wolfgang_output` where the files are: a single folder →
/// inside that folder; files (or several items) → next to the first one.
/// Every drop from the same place shares it (the run log knows the files by
/// content). A `wolfgang_output` that isn't Wolfgang's (no run log, not empty)
/// is never written into; `-2`, `-3`, … are tried instead.
fn locate_output(items: &[PathBuf]) -> Result<(PathBuf, String, RunLog)> {
    let first = &items[0];
    let base = if items.len() == 1 && first.is_dir() { first.as_path() } else { first.parent().unwrap_or(first) };
    let source_key = base.to_string_lossy().into_owned();
    for n in 1u32.. {
        let root = base.join(if n == 1 { OUTPUT_DIR.to_owned() } else { format!("{OUTPUT_DIR}-{n}") });
        if !root.exists() {
            return Ok((root, source_key, RunLog::default()));
        }
        let ours = runlog::log_path(&root).exists();
        let empty = std::fs::read_dir(&root).map(|mut d| d.next().is_none()).unwrap_or(false);
        if ours || empty {
            return Ok((root.clone(), source_key, RunLog::open(&root)?));
        }
    }
    unreachable!()
}
