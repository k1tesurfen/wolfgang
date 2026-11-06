//! Walking what was dropped on the desk and sorting it into piles.

use crate::error::{io_err, Result};
use crate::kind::{classify, FileKind};
use crate::naming;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Folder name suffix of Wolfgang's output folders; never scanned.
pub const OUTPUT_SUFFIX: &str = "-wolfgang";

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    /// Path inside the output folder's mirror, e.g. `Strand/IMG_1.heic`.
    pub rel: PathBuf,
    pub kind: FileKind,
    pub size: u64,
}

impl ScannedFile {
    /// Relative folder as a `/`-separated string (`""` = top level).
    pub fn rel_dir(&self) -> String {
        rel_dir_string(&self.rel)
    }
}

#[derive(Debug, Clone)]
pub struct KeywordFile {
    pub path: PathBuf,
    pub words: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub items: Vec<PathBuf>,
    /// Images, fonts and unknown files, sorted by `rel`. Keyword files are not listed here.
    pub files: Vec<ScannedFile>,
    /// Most recently modified `*.txt` per relative folder.
    pub keyword_files: BTreeMap<String, KeywordFile>,
}

impl Scan {
    pub fn of_kind(&self, kind: FileKind) -> impl Iterator<Item = &ScannedFile> {
        self.files.iter().filter(move |f| f.kind == kind)
    }

    pub fn count(&self, kind: FileKind) -> usize {
        self.of_kind(kind).count()
    }

    /// Folders that contain images, sorted.
    pub fn image_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self.of_kind(FileKind::Image).map(ScannedFile::rel_dir).collect();
        dirs.dedup();
        dirs.sort();
        dirs.dedup();
        dirs
    }
}

pub(crate) fn rel_dir_string(rel: &Path) -> String {
    rel.parent()
        .map(|p| p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

/// A single dropped folder mirrors its *contents*; anything else mirrors the
/// dropped items themselves (folder names included) next to each other.
pub fn scan(items: &[PathBuf]) -> Result<Scan> {
    let items: Vec<PathBuf> = items.iter().map(|p| p.canonicalize().map_err(io_err(p))).collect::<Result<_>>()?;
    let single_dir = items.len() == 1 && items[0].is_dir();

    let mut files = Vec::new();
    let mut keyword_candidates: BTreeMap<String, Vec<(SystemTime, PathBuf)>> = BTreeMap::new();

    for item in &items {
        let base = if single_dir { item.clone() } else { item.parent().map(Path::to_path_buf).unwrap_or_else(|| item.clone()) };
        let walker = WalkDir::new(item).follow_links(false).sort_by_file_name().into_iter().filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !(name.starts_with('.') || (e.file_type().is_dir() && is_output_dir_name(&name)))
        });
        for entry in walker {
            let entry = entry.map_err(|e| {
                let path = e.path().map(Path::to_path_buf).unwrap_or_default();
                io_err(&path)(e.into())
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(kind) = classify(entry.path()) else { continue };
            let meta = entry.metadata().map_err(|e| io_err(entry.path())(e.into()))?;
            let rel = entry.path().strip_prefix(&base).unwrap_or(entry.path()).to_path_buf();
            if kind == FileKind::KeywordFile {
                let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                keyword_candidates.entry(rel_dir_string(&rel)).or_default().push((mtime, entry.path().to_path_buf()));
            } else {
                files.push(ScannedFile { path: entry.path().to_path_buf(), rel, kind, size: meta.len() });
            }
        }
    }
    files.sort_by(|a, b| finder_order(&a.rel, &b.rel));

    let mut keyword_files = BTreeMap::new();
    for (dir, mut candidates) in keyword_candidates {
        candidates.sort();
        let (_, path) = candidates.pop().expect("non-empty");
        let text = std::fs::read(&path).map_err(io_err(&path))?;
        let words = naming::keyword_words(&String::from_utf8_lossy(&text));
        if !words.is_empty() {
            keyword_files.insert(dir, KeywordFile { path, words });
        }
    }

    Ok(Scan { items, files, keyword_files })
}

/// Finder-like order: case-insensitive, numbers by value (`IMG_2` before `IMG_10`).
/// Indexes are handed out in this order, so `0001` is the first file the user sees.
pub fn finder_order(a: &Path, b: &Path) -> std::cmp::Ordering {
    fn key(s: &str) -> Vec<(u8, u128, String)> {
        let mut out = Vec::new();
        let mut chars = s.chars().peekable();
        while let Some(&c) = chars.peek() {
            let digit = c.is_ascii_digit();
            let mut run = String::new();
            while let Some(&c) = chars.peek().filter(|c| c.is_ascii_digit() == digit) {
                run.push(c);
                chars.next();
            }
            out.push(if digit { (0, run.parse().unwrap_or(u128::MAX), run) } else { (1, 0, run.to_lowercase()) });
        }
        out
    }
    let comps = |p: &Path| p.components().map(|c| key(&c.as_os_str().to_string_lossy())).collect::<Vec<_>>();
    comps(a).cmp(&comps(b)).then_with(|| a.cmp(b))
}

/// `foo-wolfgang`, `foo-wolfgang-2`, …
pub(crate) fn is_output_dir_name(name: &str) -> bool {
    if name.ends_with(OUTPUT_SUFFIX) {
        return true;
    }
    match name.rsplit_once('-') {
        Some((head, n)) => head.ends_with(OUTPUT_SUFFIX) && n.parse::<u32>().is_ok(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path, content: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    #[test]
    fn single_folder_mirrors_contents_and_skips_noise() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Urlaub");
        touch(&root.join("a.JPG"), "x");
        touch(&root.join("Strand/b.heic"), "x");
        touch(&root.join("Fonts/Inter.otf"), "x");
        touch(&root.join("notes.pdf"), "x");
        touch(&root.join(".DS_Store"), "x");
        touch(&root.join(".hidden/c.jpg"), "x");
        touch(&root.join("Urlaub-wolfgang/old.webp"), "x");
        touch(&root.join("x-wolfgang-2/old.webp"), "x");

        let s = scan(&[root]).unwrap();
        let rels: Vec<_> = s.files.iter().map(|f| (f.rel.to_string_lossy().into_owned(), f.kind)).collect();
        assert_eq!(
            rels,
            vec![
                ("a.JPG".into(), FileKind::Image),
                ("Fonts/Inter.otf".into(), FileKind::Font),
                ("notes.pdf".into(), FileKind::Unknown),
                ("Strand/b.heic".into(), FileKind::Image),
            ]
        );
        assert_eq!(s.image_dirs(), vec!["".to_string(), "Strand".to_string()]);
    }

    #[test]
    fn multiple_items_keep_their_names() {
        let t = tempfile::tempdir().unwrap();
        touch(&t.path().join("one.png"), "x");
        touch(&t.path().join("Mappe/two.png"), "x");
        let s = scan(&[t.path().join("one.png"), t.path().join("Mappe")]).unwrap();
        let rels: Vec<_> = s.files.iter().map(|f| f.rel.to_string_lossy().into_owned()).collect();
        assert_eq!(rels, vec!["Mappe/two.png", "one.png"]);
    }

    #[test]
    fn newest_keyword_file_per_folder_wins() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        touch(&root.join("img.jpg"), "x");
        touch(&root.join("old.txt"), "alt\n");
        let old = filetime_past();
        fs::File::options().write(true).open(root.join("old.txt")).unwrap().set_modified(old).unwrap();
        touch(&root.join("new.txt"), "Strand Sonnenuntergang\nAlgarve\n");
        touch(&root.join("sub/img.jpg"), "x");
        touch(&root.join("sub/empty.txt"), "\n  \n");

        let s = scan(&[root]).unwrap();
        assert_eq!(s.keyword_files.len(), 1, "empty keyword files are ignored");
        assert_eq!(s.keyword_files[""].words, vec!["Strand", "Sonnenuntergang", "Algarve"]);
    }

    fn filetime_past() -> SystemTime {
        SystemTime::now() - std::time::Duration::from_secs(3600)
    }

    #[test]
    fn sorts_like_finder() {
        let mut v: Vec<PathBuf> = ["b/IMG_10.jpg", "a.jpg", "B/x.jpg", "IMG_10.jpg", "img_2.jpg", "Logo.png", "orient.jpg"].iter().map(PathBuf::from).collect();
        v.sort_by(|a, b| finder_order(a, b));
        let v: Vec<_> = v.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        assert_eq!(v, ["a.jpg", "b/IMG_10.jpg", "B/x.jpg", "img_2.jpg", "IMG_10.jpg", "Logo.png", "orient.jpg"]);
    }

    #[test]
    fn output_dir_names() {
        assert!(is_output_dir_name("Urlaub-wolfgang"));
        assert!(is_output_dir_name("Urlaub-wolfgang-12"));
        assert!(!is_output_dir_name("wolfgang"));
        assert!(!is_output_dir_name("Urlaub-wolfgang-alt"));
    }
}
