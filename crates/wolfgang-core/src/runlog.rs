//! `.wolfgang/run.jsonl` inside an output folder: one line per run and per
//! converted file. It remembers which sources (by content hash) were already
//! converted and which index each folder reached.

use crate::error::{io_err, Result};
use crate::settings::RunSettings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const LOG_DIR: &str = ".wolfgang";
pub const LOG_FILE: &str = "run.jsonl";
pub const LOG_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Entry {
    Run {
        version: u32,
        started: chrono::DateTime<chrono::Local>,
        /// Identifies what was dropped, so a later drop can find this folder again.
        source: String,
        settings: RunSettings,
    },
    File(FileRecord),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// BLAKE3 of the source file's content.
    pub hash: String,
    /// Source path relative to the mirror root.
    pub source: String,
    /// Relative source folder the index counter belongs to.
    pub dir: String,
    pub index: Option<u32>,
    /// Written files, relative to the output folder.
    pub outputs: Vec<String>,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Debug, Default)]
pub struct RunLog {
    pub source: Option<String>,
    pub runs: usize,
    /// Latest record per source hash.
    pub files: HashMap<String, FileRecord>,
    /// Highest index used per relative folder.
    pub max_index: HashMap<String, u32>,
    /// Lines that could not be parsed (ignored, but counted).
    pub broken_lines: usize,
}

pub fn log_path(output_root: &Path) -> PathBuf {
    output_root.join(LOG_DIR).join(LOG_FILE)
}

impl RunLog {
    /// Reads the log of an output folder; a missing log is an empty one.
    pub fn open(output_root: &Path) -> Result<Self> {
        let path = log_path(output_root);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(io_err(&path)(e)),
        };
        let mut log = Self::default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            match serde_json::from_str::<Entry>(line) {
                Ok(Entry::Run { source, .. }) => {
                    log.runs += 1;
                    log.source.get_or_insert(source);
                }
                Ok(Entry::File(rec)) => {
                    if let Some(i) = rec.index {
                        let max = log.max_index.entry(rec.dir.clone()).or_insert(0);
                        *max = (*max).max(i);
                    }
                    log.files.insert(rec.hash.clone(), rec);
                }
                Err(_) => log.broken_lines += 1,
            }
        }
        Ok(log)
    }
}

/// Appends entries line by line; safe to share between worker threads.
pub struct LogWriter {
    path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl LogWriter {
    pub fn open(output_root: &Path) -> Result<Self> {
        let path = log_path(output_root);
        let dir = path.parent().expect("has parent");
        std::fs::create_dir_all(dir).map_err(io_err(dir))?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(io_err(&path))?;
        Ok(Self { path, file: Mutex::new(file) })
    }

    pub fn append(&self, entry: &Entry) -> Result<()> {
        let mut line = serde_json::to_string(entry).expect("entries serialize");
        line.push('\n');
        let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
        f.write_all(line.as_bytes()).and_then(|_| f.flush()).map_err(io_err(&self.path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(hash: &str, dir: &str, index: u32) -> Entry {
        Entry::File(FileRecord {
            hash: hash.into(),
            source: format!("{dir}/x.jpg"),
            dir: dir.into(),
            index: Some(index),
            outputs: vec![format!("{dir}/x-{index:04}.webp")],
            bytes_in: 10,
            bytes_out: 2,
        })
    }

    #[test]
    fn writes_and_reads_back() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(RunLog::open(t.path()).unwrap().runs, 0);

        let w = LogWriter::open(t.path()).unwrap();
        w.append(&Entry::Run { version: LOG_VERSION, started: chrono::Local::now(), source: "/x".into(), settings: RunSettings::default() })
            .unwrap();
        w.append(&rec("aa", "", 1)).unwrap();
        w.append(&rec("bb", "", 2)).unwrap();
        w.append(&rec("cc", "sub", 1)).unwrap();
        w.append(&rec("aa", "", 5)).unwrap(); // re-converted later
        drop(w);
        std::fs::OpenOptions::new().append(true).open(log_path(t.path())).unwrap().write_all(b"{nonsense\n").unwrap();

        let log = RunLog::open(t.path()).unwrap();
        assert_eq!(log.runs, 1);
        assert_eq!(log.source.as_deref(), Some("/x"));
        assert_eq!(log.files.len(), 3);
        assert_eq!(log.files["aa"].index, Some(5));
        assert_eq!(log.max_index[""], 5);
        assert_eq!(log.max_index["sub"], 1);
        assert_eq!(log.broken_lines, 1);
    }
}
