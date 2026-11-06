//! Executing a [`Plan`]: parallel conversion with progress events, cancellation
//! and an append-only run log.

use crate::error::{io_err, Error, Result};
use crate::font::{self, Woff2Mode};
use crate::image;
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
    FileDone(FileResult),
}

#[derive(Debug, Clone)]
pub struct FileResult {
    pub rel: PathBuf,
    pub target: PathBuf,
    pub bytes_in: u64,
    /// `Err` holds a user-facing message.
    pub outcome: std::result::Result<Done, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub bytes_out: u64,
    /// Something worth mentioning, e.g. a font that needed the safe WOFF2 path.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub output_root: PathBuf,
    pub converted: usize,
    pub failed: Vec<(PathBuf, String)>,
    pub notes: Vec<(PathBuf, String)>,
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
            let outcome = convert_job(job, &plan.output_root).map_err(|e| e.to_string());
            if let Ok(done) = &outcome {
                let rec = FileRecord {
                    hash: job.hash.clone(),
                    source: rel_string(&job.rel),
                    dir: job.dir.clone(),
                    index: job.index,
                    outputs: vec![rel_string(&job.target)],
                    bytes_in: job.size,
                    bytes_out: done.bytes_out,
                };
                if let Err(e) = log.append(&Entry::File(rec)) {
                    log_error.lock().unwrap().get_or_insert(e);
                }
            }
            let result = FileResult { rel: job.rel.clone(), target: job.target.clone(), bytes_in: job.size, outcome };
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
                report.bytes_in += r.bytes_in;
                report.bytes_out += done.bytes_out;
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

fn convert_job(job: &Job, output_root: &Path) -> Result<Done> {
    let (bytes, note) = match &job.task {
        Task::Image(task) => (image::convert(&job.source, task)?, None),
        Task::Font => {
            let data = std::fs::read(&job.source).map_err(io_err(&job.source))?;
            let w = font::to_woff2(&font::to_sfnt(&data)?)?;
            let note = (w.mode == Woff2Mode::Untransformed).then(|| "Schrift vorsichtshalber ohne Glyphen-Umbau gepackt (etwas größer, dafür 1:1)".to_owned());
            (w.data, note)
        }
    };
    let target = output_root.join(&job.target);
    write_atomic(&target, &bytes)?;
    Ok(Done { bytes_out: bytes.len() as u64, note })
}

/// Writes to a hidden temp file next to the target, then renames:
/// a cancelled or crashed run never leaves half-written files behind.
fn write_atomic(target: &Path, bytes: &[u8]) -> Result<()> {
    let dir = target.parent().expect("target has a folder");
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    let tmp = dir.join(format!(".{}.part", target.file_name().unwrap().to_string_lossy()));
    std::fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
    std::fs::rename(&tmp, target).map_err(io_err(target))
}
