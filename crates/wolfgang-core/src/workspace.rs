//! A drop on Wolfgang's desk: what's there, where the results go, what's new,
//! and — once settings are known — the concrete list of jobs.

use crate::error::{io_err, Result};
use crate::font;
use crate::image::{ColorTarget, ImageTask};
use crate::kind::FileKind;
use crate::media::{self, VideoTask};
use crate::naming;
use crate::runlog::{self, RunLog};
use crate::scan::{self, Scan, ScannedFile, OUTPUT_SUFFIX};
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
    /// Where results go, e.g. `~/Bilder/Urlaub-wolfgang`.
    pub output_root: PathBuf,
    /// Identifies the drop so a later drop of the same thing finds this folder.
    pub source_key: String,
    pub log: RunLog,
    /// Content hash per entry of `scan.files` (unknown files get an empty hash).
    hashes: Vec<String>,
    /// Per entry of `scan.files`: does the video have sound? (`None` for
    /// anything else, or if it couldn't be looked at.)
    has_sound: Vec<Option<bool>>,
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
    pub fonts: bool,
}

impl Piles {
    /// Images, videos or audio: everything that has questions.
    pub fn media(self) -> bool {
        self.images || self.videos || self.audio
    }
}

impl Summary {
    /// Videos and audio only count if the tools to convert them are there.
    pub fn piles(&self) -> Piles {
        let tools = !self.media_tools_missing;
        Piles { images: self.images.total > 0, videos: tools && self.videos.total > 0, audio: tools && self.audio.total > 0, fonts: self.fonts.total > 0 }
    }

    /// All piles that get converted, for totals.
    pub fn convertible(&self) -> Vec<&PileSummary> {
        let p = self.piles();
        [(p.images, &self.images), (p.videos, &self.videos), (p.audio, &self.audio), (p.fonts, &self.fonts)].into_iter().filter(|(on, _)| *on).map(|(_, s)| s).collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub images: PileSummary,
    pub videos: PileSummary,
    pub audio: PileSummary,
    pub fonts: PileSummary,
    /// Videos with a sound track (or that couldn't be checked).
    pub videos_with_sound: usize,
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
        let scan = scan::scan(items)?;
        let (output_root, source_key, log) = locate_output(&scan.items)?;
        let hashes = scan
            .files
            .par_iter()
            .map(|f| if f.kind == FileKind::Unknown { Ok(String::new()) } else { hash_file(&f.path) })
            .collect::<Result<Vec<_>>>()?;
        // Only to know whether to ask about sound; a broken file is dealt with later.
        let has_sound = scan
            .files
            .par_iter()
            .map(|f| (f.kind == FileKind::Video && media::tools().is_some()).then(|| media::probe(&f.path).map(|i| i.audio.is_some()).unwrap_or(true)))
            .collect();
        Ok(Self { scan, output_root, source_key, log, hashes, has_sound })
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
        self.has_sound.retain(|_| *k.next().unwrap());
        if self.scan.count(FileKind::Image) == 0 {
            self.scan.keyword_files.clear();
        }
    }

    /// Known kinds with their hash and (videos only) whether they have sound.
    fn convertible(&self) -> impl Iterator<Item = (&ScannedFile, &String, Option<bool>)> {
        self.scan.files.iter().zip(&self.hashes).zip(&self.has_sound).filter(|((f, _), _)| f.kind != FileKind::Unknown).map(|((f, h), s)| (f, h, *s))
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary { previous_runs: self.log.runs, ..Default::default() };
        let mut seen = HashSet::new();
        for (f, hash, sound) in self.convertible() {
            if !seen.insert(hash) {
                s.duplicates += 1;
                continue;
            }
            let pile = match f.kind {
                FileKind::Image => &mut s.images,
                FileKind::Video => {
                    s.videos_with_sound += usize::from(sound.unwrap_or(true));
                    &mut s.videos
                }
                FileKind::Audio => &mut s.audio,
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
            };
            let poster = matches!(&job.task, Task::Video(t) if t.settings.poster);
            let exts: &[&str] = if poster { &[ext, POSTER_EXT] } else { &[ext] };
            let original_ext = f.rel.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let name = self.free_name(&folder, &stem, &original_ext, exts, &mut taken, &previous_outputs);
            job.target = folder.join(format!("{name}.{ext}"));
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
            let rels: Vec<String> = exts.iter().map(|ext| rel_string(&folder.join(format!("{name}.{ext}")))).collect();
            let free = rels.iter().all(|rel| {
                let on_disk = self.output_root.join(rel).exists() && !own.contains(rel.as_str());
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

/// A single folder → `<folder>-wolfgang` next to it. Anything else →
/// `<parent>-wolfgang` inside the first item's parent. If such a folder exists
/// but belongs to a different source, `-2`, `-3`, … are tried.
fn locate_output(items: &[PathBuf]) -> Result<(PathBuf, String, RunLog)> {
    let first = &items[0];
    let parent = first.parent().unwrap_or(first);
    let (base_name, source_key) = if items.len() == 1 && first.is_dir() {
        (dir_name(first), first.to_string_lossy().into_owned())
    } else {
        (dir_name(parent), format!("files in {}", parent.to_string_lossy()))
    };
    for n in 1u32.. {
        let name = if n == 1 { format!("{base_name}{OUTPUT_SUFFIX}") } else { format!("{base_name}{OUTPUT_SUFFIX}-{n}") };
        let root = parent.join(name);
        if !root.exists() {
            return Ok((root, source_key, RunLog::default()));
        }
        let log = RunLog::open(&root)?;
        if log.source.as_deref() == Some(source_key.as_str()) {
            return Ok((root, source_key, log));
        }
        // An empty folder we may have created earlier without a log is fine to reuse.
        if !runlog::log_path(&root).exists() && std::fs::read_dir(&root).map(|mut d| d.next().is_none()).unwrap_or(false) {
            return Ok((root, source_key, log));
        }
    }
    unreachable!()
}

fn dir_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Ablage".into())
}
