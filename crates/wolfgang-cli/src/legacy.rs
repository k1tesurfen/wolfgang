//! `wolfgang legacy-reset`: undo the in-place renames of Wolfgang 1.x
//! (`DSC_4711.jpg` → `DSC_4711-0001.jpg`).
//!
//! Unlike the old `--reset`, which renamed *any* file ending in `-NNNN`,
//! this only touches files the old run log recorded, with their exact index.

use std::path::{Path, PathBuf};

pub const OLD_LOG: &str = ".wolfgang_run_log.jsonl";

#[derive(Debug, Default, PartialEq)]
pub struct ResetPlan {
    /// (current path, original path)
    pub renames: Vec<(PathBuf, PathBuf)>,
    /// Recorded files that no longer exist under their indexed name.
    pub missing: Vec<PathBuf>,
    /// Original name is taken by another file.
    pub blocked: Vec<(PathBuf, PathBuf)>,
}

/// Reads the old log. It was written with `jq -n`, i.e. as a stream of
/// pretty-printed JSON objects rather than one object per line.
pub fn plan(dir: &Path) -> Result<Option<ResetPlan>, String> {
    let log = dir.join(OLD_LOG);
    let text = match std::fs::read_to_string(&log) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", log.display())),
    };
    let mut plan = ResetPlan::default();
    let mut seen = std::collections::HashSet::new();
    for entry in serde_json::Deserializer::from_str(&text).into_iter::<serde_json::Value>() {
        let entry = entry.map_err(|e| format!("Altes Logbuch ist kaputt: {e}"))?;
        for f in entry["converted_files"].as_array().into_iter().flatten() {
            let Some(index) = f["filename_index"].as_str() else { continue };
            // Prefer the relative path: the folder may have moved since.
            let current = match (f["original_relative_path"].as_str(), f["original_path"].as_str()) {
                (Some(rel), _) if dir.join(rel).exists() => dir.join(rel),
                (_, Some(abs)) if Path::new(abs).exists() => PathBuf::from(abs),
                (Some(rel), _) => dir.join(rel),
                (None, Some(abs)) => PathBuf::from(abs),
                (None, None) => continue,
            };
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(original) = strip_index(&current, index) else { continue };
            if !current.exists() {
                plan.missing.push(current);
            } else if original.exists() {
                plan.blocked.push((current, original));
            } else {
                plan.renames.push((current, original));
            }
        }
    }
    Ok(Some(plan))
}

/// `DSC_4711-0001.jpg` + `0001` → `DSC_4711.jpg`
fn strip_index(path: &Path, index: &str) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let base = stem.strip_suffix(&format!("-{index}"))?;
    let name = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{base}.{ext}"),
        None => base.to_owned(),
    };
    Some(path.with_file_name(name))
}

/// Renames and, if everything went through, retires the old log
/// (kept as `.wolfgang_run_log.jsonl.done` for reference).
pub fn apply(dir: &Path, plan: &ResetPlan) -> Vec<(PathBuf, String)> {
    let mut errors = Vec::new();
    for (from, to) in &plan.renames {
        if let Err(e) = std::fs::rename(from, to) {
            errors.push((from.clone(), e.to_string()));
        }
    }
    if errors.is_empty() && plan.blocked.is_empty() {
        let log = dir.join(OLD_LOG);
        let _ = std::fs::rename(&log, log.with_extension("jsonl.done"));
    }
    errors
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A log as Wolfgang 1.x wrote it (pretty-printed objects back to back).
    pub fn write_old_log(dir: &Path, files: &[(&str, &str)]) {
        let mut text = String::new();
        for (rel, idx) in files {
            text.push_str(&serde_json::to_string_pretty(&serde_json::json!({
                "run_timestamp": "2025-05-01T10:00:00",
                "settings": { "max_size": 1400 },
                "converted_files": [{
                    "original_path": dir.join(rel),
                    "original_relative_path": rel,
                    "filename_index": idx,
                }]
            })).unwrap());
            text.push('\n');
        }
        std::fs::write(dir.join(OLD_LOG), text).unwrap();
    }

    #[test]
    fn only_logged_files_are_renamed() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for f in ["DSC_1-0001.jpg", "sub/b-0002.png", "Sommer-2023.jpg", "c-0003.jpg", "c.jpg"] {
            std::fs::write(d.join(f), "x").unwrap();
        }
        write_old_log(d, &[("DSC_1-0001.jpg", "0001"), ("sub/b-0002.png", "0002"), ("c-0003.jpg", "0003"), ("gone-0004.jpg", "0004")]);

        let p = plan(d).unwrap().unwrap();
        assert_eq!(p.renames, vec![(d.join("DSC_1-0001.jpg"), d.join("DSC_1.jpg")), (d.join("sub/b-0002.png"), d.join("sub/b.png"))]);
        assert_eq!(p.blocked, vec![(d.join("c-0003.jpg"), d.join("c.jpg"))]);
        assert_eq!(p.missing, vec![d.join("gone-0004.jpg")]);

        assert!(apply(d, &p).is_empty());
        assert!(d.join("DSC_1.jpg").exists() && d.join("sub/b.png").exists());
        assert!(d.join("Sommer-2023.jpg").exists(), "never touched: not in the log");
        assert!(d.join(OLD_LOG).exists(), "log kept while something is blocked");
    }

    #[test]
    fn no_log_no_plan() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(plan(t.path()).unwrap(), None);
    }
}
