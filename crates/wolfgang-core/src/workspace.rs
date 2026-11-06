//! A drop on Wolfgang's desk: what's there, where the results go, what's new,
//! and — once settings are known — the concrete list of jobs.

use crate::error::{io_err, Result};
use crate::font;
use crate::image::{ColorTarget, ImageTask};
use crate::kind::FileKind;
use crate::naming;
use crate::runlog::{self, RunLog};
use crate::scan::{self, Scan, ScannedFile, OUTPUT_SUFFIX};
use crate::settings::{self, ImageSettings, Naming, RunSettings};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Output folder for font results, inside the output root.
pub const FONTS_DIR: &str = "fonts";

pub struct Workspace {
    pub scan: Scan,
    /// Where results go, e.g. `~/Bilder/Urlaub-wolfgang`.
    pub output_root: PathBuf,
    /// Identifies the drop so a later drop of the same thing finds this folder.
    pub source_key: String,
    pub log: RunLog,
    /// Content hash per entry of `scan.files` (unknown files get an empty hash).
    hashes: Vec<String>,
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

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub images: PileSummary,
    pub fonts: PileSummary,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
        Ok(Self { scan, output_root, source_key, log, hashes })
    }

    fn convertible(&self) -> impl Iterator<Item = (&ScannedFile, &String)> {
        self.scan.files.iter().zip(&self.hashes).filter(|(f, _)| f.kind != FileKind::Unknown)
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary { previous_runs: self.log.runs, ..Default::default() };
        let mut seen = HashSet::new();
        for (f, hash) in self.convertible() {
            if !seen.insert(hash) {
                s.duplicates += 1;
                continue;
            }
            let pile = if f.kind == FileKind::Image { &mut s.images } else { &mut s.fonts };
            pile.total += 1;
            pile.bytes += f.size;
            if self.log.files.contains_key(hash) {
                pile.known += 1;
            }
        }
        s.unknown = self.scan.of_kind(FileKind::Unknown).map(|f| f.rel.clone()).collect();
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

        for (f, hash) in self.convertible() {
            let wanted = match f.kind {
                FileKind::Image => settings.images.is_some(),
                FileKind::Font => settings.fonts.is_some(),
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
            };
            let original_ext = f.rel.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            job.target = self.free_target(&folder, &stem, &original_ext, ext, &mut taken, &previous_outputs);
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

    /// `stem.ext`, or `stem-<origext>.ext` / `stem-2.ext` … if that's taken
    /// in this run or by a file on disk that isn't this source's own earlier output.
    fn free_target(&self, folder: &Path, stem: &str, original_ext: &str, ext: &str, taken: &mut HashSet<String>, own: &HashSet<&str>) -> PathBuf {
        let candidates = std::iter::once(stem.to_owned())
            .chain((!original_ext.is_empty()).then(|| format!("{stem}-{original_ext}")))
            .chain((2..).map(|n| format!("{stem}-{n}")));
        for name in candidates {
            let rel = folder.join(format!("{name}.{ext}"));
            let rel_str = rel_string(&rel);
            let on_disk = self.output_root.join(&rel).exists() && !own.contains(rel_str.as_str());
            if !on_disk && taken.insert(rel_str.to_lowercase()) {
                return rel;
            }
        }
        unreachable!("infinite candidates")
    }
}

pub(crate) fn rel_string(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
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
