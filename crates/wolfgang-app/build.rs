//! Copies the pixel art from `assets/pixel` into `ui/pixel` (what the app
//! serves), then lets Tauri do its build-time work.

use std::path::Path;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            // edit/: local working copies for painting (wolfgang-art to-png), not app art.
            if name != "edit" {
                copy_dir(&path, &to.join(&name));
            }
        } else if path.extension().is_some_and(|e| e == "png" || e == "json") {
            std::fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn main() {
    let pixel = Path::new("../../assets/pixel");
    println!("cargo:rerun-if-changed={}", pixel.display());
    let target = Path::new("ui/pixel");
    let _ = std::fs::remove_dir_all(target);
    copy_dir(pixel, target);
    tauri_build::build();
}
