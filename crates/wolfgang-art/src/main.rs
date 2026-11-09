//! `wolfgang-art`: builds the app's pixel art from `assets/`.
//!
//!   cargo run --release -p wolfgang-art -- build            hand-made sources → sheets, scene.json, preview
//!   cargo run --release -p wolfgang-art -- import [--force] references → src/ PNGs (once!)
//!   cargo run --release -p wolfgang-art -- analyze IMAGE... pixel grid detection

mod build;
mod grid;
mod pixelize;
mod sprite;

use std::path::Path;
use wolfgang_core::image::{decode, ColorTarget, Pixels};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("analyze") => {
            for path in &args[1..] {
                let img = decode(Path::new(path), None, ColorTarget::Srgb).expect("decode");
                let full = (0, 0, img.width, img.height);
                let g = grid::find(&img, full, 3.0, 30.0);
                println!(
                    "{path}: {}x{}  x: period {:.2} phase {:.2} conf {:.3}  y: period {:.2} phase {:.2} conf {:.3}  → {:.0}x{:.0} art px",
                    img.width,
                    img.height,
                    g.x.period,
                    g.x.phase,
                    g.x.confidence,
                    g.y.period,
                    g.y.phase,
                    g.y.confidence,
                    img.width as f64 / g.x.period,
                    img.height as f64 / g.y.period
                );
                // Local consistency: the same detection on four quadrants.
                let (w, h) = (img.width / 2, img.height / 2);
                for (qx, qy) in [(0, 0), (w, 0), (0, h), (w, h)] {
                    let q = grid::find(&img, (qx, qy, w, h), 3.0, 30.0);
                    println!("    quadrant {qx},{qy}: x {:.2}/{:.2}  y {:.2}/{:.2}", q.x.period, q.x.phase, q.y.period, q.y.phase);
                }
            }
        }
        Some("import") => {
            // import [--force]: references → src/office.png + src/portraits/*.png, once.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            if let Err(e) = build::import(&paths, args.iter().any(|a| a == "--force")) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Some("icon") => {
            // icon OUT.png: 1024 px app icon from the happy portrait.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            build::icon(&paths, Path::new(args.get(1).map(String::as_str).unwrap_or("icon.png")));
        }
        Some("build") => {
            // build [--preview DIR]
            let preview = args.iter().position(|a| a == "--preview").map(|i| std::path::PathBuf::from(&args[i + 1]));
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview };
            println!("assets/pixel/");
            build::check_sources(&paths);
            let wolfgang = build::sprites(&paths, "wolfgang");
            let ui = build::sprites(&paths, "ui");
            let wilma = build::sprites(&paths, "wilma");
            build::manifest(&paths, wolfgang, ui, wilma);
        }
        _ => eprintln!("usage: wolfgang-art build [--preview DIR] | import [--force] | icon OUT.png | analyze IMAGE..."),
    }
}

/// Nearest-neighbour upscale for looking at results.
pub fn upscale(p: &Pixels, k: u32) -> Pixels {
    let (w, h) = (p.width * k, p.height * k);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let o = (((y / k) * p.width + x / k) * 4) as usize;
            rgba.extend_from_slice(&p.rgba[o..o + 4]);
        }
    }
    Pixels { width: w, height: h, rgba, icc: None }
}
