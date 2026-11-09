//! Executing a [`Plan`]: parallel conversion with progress events, cancellation
//! and an append-only run log.

use crate::error::{io_err, Error, Result};
use crate::font::{self, Woff2Mode};
use crate::image;
use crate::media;
use crate::pdf;
use crate::runlog::{Entry, FileRecord, LogWriter, LOG_VERSION};
use crate::workspace::{rel_string, Job, Plan, Task};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Event {
    Started { total: usize, output_root: PathBuf },
    FileStarted { number: usize, total: usize, rel: PathBuf },
    /// How far a long file (video, audio) is, 0–1. Not sent for images and fonts.
    FileProgress { number: usize, rel: PathBuf, fraction: f64 },
    FileDone(FileResult),
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub rel: PathBuf,
    pub target: PathBuf,
    pub bytes_in: u64,
    /// Pictures pulled out of a PDF: not a conversion, so it doesn't count as „saved“.
    pub extracted: bool,
    /// `Err` holds a user-facing message.
    pub outcome: std::result::Result<Done, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub bytes_out: u64,
    /// Something worth mentioning, e.g. a font that needed the safe WOFF2 path.
    pub note: Option<Note>,
    /// Written files, relative to the output root.
    pub outputs: Vec<PathBuf>,
}

/// Something Wolfgang mentions after the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Note {
    /// A font was packed without the WOFF2 glyph transform (a bit bigger, but 1:1).
    FontUntransformed,
    /// The video was already well packed; the original was taken, only tidied up.
    VideoKeptOriginal,
    /// The converted video is bigger than the original.
    VideoGrew,
    /// A PDF without any pictures in it.
    PdfNoPictures,
    /// Some pictures in a PDF are in formats Wolfgang can't read.
    PdfSomeUnreadable,
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::FontUntransformed => "Schrift vorsichtshalber ohne Glyphen-Umbau gepackt (etwas größer, dafür 1:1)",
            Self::VideoKeptOriginal => "war schon gut gepackt – das Original genommen, nur aufgeräumt",
            Self::VideoGrew => "ist größer geworden als vorher, war wohl schon gut gepackt",
            Self::PdfNoPictures => "da war gar kein Bild drin",
            Self::PdfSomeUnreadable => "ein paar Bilder darin kann ich nicht lesen (z. B. Fax-Scans)",
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub output_root: PathBuf,
    pub converted: usize,
    pub failed: Vec<(PathBuf, String)>,
    pub notes: Vec<(PathBuf, Note)>,
    /// Pictures pulled out of PDFs (not part of `bytes_in`/`bytes_out`).
    pub pdf_pictures: usize,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub duration: Duration,
    pub cancelled: bool,
}

/// Converts all jobs. Individual failures don't stop the run; they end up in the report.
/// `cancel` stops before the next file starts.
pub fn execute(plan: &Plan, on_event: &(dyn Fn(Event) + Sync), cancel: &AtomicBool) -> Result<Report> {
    let started = Instant::now();
    let total = plan.jobs.len();
    on_event(Event::Started { total, output_root: plan.output_root.clone() });

    let mut report = Report { output_root: plan.output_root.clone(), ..Default::default() };
    if total == 0 {
        return Ok(report);
    }
    std::fs::create_dir_all(&plan.output_root).map_err(io_err(&plan.output_root))?;
    let log = LogWriter::open(&plan.output_root)?;
    log.append(&Entry::Run { version: LOG_VERSION, started: chrono::Local::now(), source: plan.source_key.clone(), settings: plan.settings.clone() })?;

    // Big images take a lot of memory while decoding; keep parallelism modest.
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 4);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().map_err(|e| Error::Settings(e.to_string()))?;

    let counter = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(total));
    let log_error = Mutex::new(None);

    pool.install(|| {
        plan.jobs.par_iter().for_each(|job| {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let number = counter.fetch_add(1, Ordering::Relaxed) + 1;
            on_event(Event::FileStarted { number, total, rel: job.rel.clone() });
            let progress = |fraction: f64| on_event(Event::FileProgress { number, rel: job.rel.clone(), fraction });
            let outcome = match convert_job(job, &plan.output_root, &progress, cancel) {
                Err(Error::Cancelled) => return, // stopped halfway: neither done nor failed
                other => other.map_err(|e| e.to_string()),
            };
            if let Ok(done) = &outcome {
                let rec = FileRecord {
                    hash: job.hash.clone(),
                    source: rel_string(&job.rel),
                    dir: job.dir.clone(),
                    index: job.index,
                    outputs: done.outputs.iter().map(|p| rel_string(p)).collect(),
                    bytes_in: job.size,
                    bytes_out: done.bytes_out,
                };
                if let Err(e) = log.append(&Entry::File(rec)) {
                    log_error.lock().unwrap().get_or_insert(e);
                }
            }
            let extracted = matches!(job.task, Task::Pdf);
            let result = FileResult { rel: job.rel.clone(), target: job.target.clone(), bytes_in: job.size, extracted, outcome };
            on_event(Event::FileDone(result.clone()));
            results.lock().unwrap().push(result);
        });
    });

    if let Some(e) = log_error.into_inner().unwrap() {
        return Err(e);
    }
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.rel.cmp(&b.rel));
    for r in results {
        match r.outcome {
            Ok(done) => {
                report.converted += 1;
                if r.extracted {
                    report.pdf_pictures += done.outputs.len();
                } else {
                    report.bytes_in += r.bytes_in;
                    report.bytes_out += done.bytes_out;
                }
                if let Some(note) = done.note {
                    report.notes.push((r.rel, note));
                }
            }
            Err(msg) => report.failed.push((r.rel, msg)),
        }
    }
    report.cancelled = cancel.load(Ordering::Relaxed) && report.converted + report.failed.len() < total;
    report.duration = started.elapsed();
    Ok(report)
}

fn convert_job(job: &Job, output_root: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<Done> {
    let target = output_root.join(&job.target);
    let outputs = || job.outputs().into_iter().map(Path::to_path_buf).collect();
    let (bytes, note) = match &job.task {
        Task::Pdf => return extract_pdf(job, output_root, progress, cancel),
        // ffmpeg writes the file itself, to a temp name that's renamed at the end.
        Task::Video(task) => {
            let tmp = temp_path(&target)?;
            let result = media::convert_video(&job.source, task, &tmp, progress, cancel)?;
            let mut bytes_out = finish(&tmp, &target)?;
            if let (Some(rel), Some(data)) = (&task.poster, result.poster) {
                write_atomic(&output_root.join(rel), &data)?;
                bytes_out += data.len() as u64;
            }
            let note = if result.kept_original {
                Some(Note::VideoKeptOriginal)
            } else {
                result.grew.then_some(Note::VideoGrew)
            };
            return Ok(Done { bytes_out, note, outputs: outputs() });
        }
        Task::Audio(settings) => {
            let tmp = temp_path(&target)?;
            media::convert_audio(&job.source, settings, &tmp, progress, cancel)?;
            return Ok(Done { bytes_out: finish(&tmp, &target)?, note: None, outputs: outputs() });
        }
        Task::Image(task) => (image::convert(&job.source, task)?, None),
        Task::Font => {
            let data = std::fs::read(&job.source).map_err(io_err(&job.source))?;
            let w = font::to_woff2(&font::to_sfnt(&data)?)?;
            let note = (w.mode == Woff2Mode::Untransformed).then_some(Note::FontUntransformed);
            (w.data, note)
        }
    };
    write_atomic(&target, &bytes)?;
    Ok(Done { bytes_out: bytes.len() as u64, note, outputs: outputs() })
}

/// `Broschüre.pdf` → `broschuere/seite-03-1.jpg`, … (pages zero-padded so they sort).
fn extract_pdf(job: &Job, output_root: &Path, progress: &(dyn Fn(f64) + Sync), cancel: &AtomicBool) -> Result<Done> {
    let found = pdf::extract(&job.source, progress, cancel)?;
    let digits = found.pages.to_string().len().max(2);
    let mut done = Done { bytes_out: 0, note: None, outputs: Vec::new() };
    for p in &found.pictures {
        let rel = job.target.join(format!("seite-{:0digits$}-{}.{}", p.page, p.number, p.ext));
        write_atomic(&output_root.join(&rel), &p.data)?;
        done.bytes_out += p.data.len() as u64;
        done.outputs.push(rel);
    }
    done.note = if found.pictures.is_empty() {
        Some(Note::PdfNoPictures)
    } else {
        (found.unsupported > 0).then_some(Note::PdfSomeUnreadable)
    };
    Ok(done)
}

/// Writes to a hidden temp file next to the target, then renames:
/// a cancelled or crashed run never leaves half-written files behind.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = temp_path(target)?;
    std::fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
    finish(&tmp, target).map(|_| ())
}

/// `.name.part` next to the target (its folder is created).
fn temp_path(target: &Path) -> Result<PathBuf> {
    let dir = target.parent().expect("target has a folder");
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    Ok(dir.join(format!(".{}.part", target.file_name().unwrap().to_string_lossy())))
}

/// Moves a finished temp file into place; returns its size.
fn finish(tmp: &Path, target: &Path) -> Result<u64> {
    let size = std::fs::metadata(tmp).map_err(io_err(tmp))?.len();
    std::fs::rename(tmp, target).map_err(io_err(target))?;
    Ok(size)
}
