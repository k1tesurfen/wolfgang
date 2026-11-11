//! `wolfgang-art`: builds the app's pixel art from `assets/`.
//!
//!   cargo run --release -p wolfgang-art -- build                 sources → sheets, scene.json, previews
//!   cargo run --release -p wolfgang-art -- import [--force]      Wolfgang's references → office + his portraits (once!)
//!   cargo run --release -p wolfgang-art -- import-character KEY [--force]
//!                                                                 assets/reference/KEY/<mood>.jpeg → KEY's portraits
//!   cargo run --release -p wolfgang-art -- keep-png KEY / keep-text KEY
//!                                                                 desk.png and desk.sprite both changed: keep one
//!   cargo run --release -p wolfgang-art -- to-png NAME           src/NAME.sprite (ui, wilma) → edit/NAME.png for painting
//!   cargo run --release -p wolfgang-art -- from-png NAME         edit/NAME.png → back into src/NAME.sprite
//!   cargo run --release -p wolfgang-art -- compare DIR IMAGE...  old vs cell-based portrait conversion, side by side
//!   cargo run --release -p wolfgang-art -- downscale IMAGE OUT.png PIXEL [COLOURS]
//!                                                                 a reference at another pixel size (sprite drawing aid)
//!   cargo run --release -p wolfgang-art -- analyze IMAGE...      pixel grid detection

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
            // import [--force]: Wolfgang's references → src/office.png + his portraits, once.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            if let Err(e) = build::import(&paths, args.iter().any(|a| a == "--force")) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Some("icon") => {
            // icon OUT.png [KEY]: 1024 px app icon from the character's happy portrait.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            build::icon(&paths, args.get(2).map(String::as_str).unwrap_or("wolfgang"), Path::new(args.get(1).map(String::as_str).unwrap_or("icon.png")));
        }
        Some("import-character") => {
            // import-character KEY [--force]: references → the character's portraits.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            let key = args.get(1).filter(|a| !a.starts_with("--")).unwrap_or_else(|| usage());
            build::import_character(&paths, key, args.iter().any(|a| a == "--force")).unwrap_or_else(|e| fail(&e));
        }
        Some(cmd @ ("keep-png" | "keep-text")) => {
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            let key = args.get(1).unwrap_or_else(|| usage());
            build::keep_desk(&paths, key, cmd == "keep-png").unwrap_or_else(|e| fail(&e));
        }
        Some("to-png") => {
            // to-png NAME [--force]: src/NAME.sprite → edit/NAME.png for a pixel editor.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            let name = args.get(1).filter(|a| !a.starts_with("--")).unwrap_or_else(|| usage());
            match build::sprite_to_png(&paths, name, args.iter().any(|a| a == "--force")) {
                Ok(out) => println!("  {}  (paint, save, then: from-png {name})", out.display()),
                Err(e) => fail(&e),
            }
        }
        Some("from-png") => {
            // from-png NAME: edit/NAME.png → src/NAME.sprite.
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview: None };
            let name = args.get(1).unwrap_or_else(|| usage());
            match build::sprite_from_png(&paths, name) {
                Ok(a) => {
                    println!("  {name}.sprite: {} pixels changed", a.changed);
                    for (ch, [r, g, b]) in &a.new_colors {
                        println!("  new colour '{ch}' {r:02x}{g:02x}{b:02x}");
                    }
                    for w in &a.warnings {
                        println!("  warning: {w}");
                    }
                }
                Err(e) => fail(&e),
            }
        }
        Some("compare") => {
            // compare OUT_DIR IMAGE...: current portrait conversion next to the cell-based one.
            let out = Path::new(args.get(1).unwrap_or_else(|| usage()));
            for path in &args[2..] {
                build::compare(Path::new(path), out).unwrap_or_else(|e| fail(&e));
            }
        }
        Some("downscale") => {
            // downscale IMAGE OUT.png PIXEL [COLOURS]: a reference at sprite scale, as a drawing aid.
            let (img, out) = (args.get(1).unwrap_or_else(|| usage()), args.get(2).unwrap_or_else(|| usage()));
            let pixel: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or_else(|| usage());
            let colors = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(32);
            build::downscale(Path::new(img), pixel, colors, Path::new(out)).unwrap_or_else(|e| fail(&e));
        }
        Some("build") => {
            // build [--preview DIR]
            let preview = args.iter().position(|a| a == "--preview").map(|i| std::path::PathBuf::from(&args[i + 1]));
            let paths = build::Paths { reference: "assets/reference".into(), out: "assets/pixel".into(), preview };
            println!("assets/pixel/");
            build::clean_portraits(&paths);
            build::check_sources(&paths);
            let characters = build::characters(&paths);
            let ui = build::sprites(&paths, "ui");
            let wilma = build::sprites(&paths, "wilma");
            build::manifest(&paths, characters, ui, wilma);
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    fail("usage: wolfgang-art build [--preview DIR] | import [--force] | import-character KEY [--force] | keep-png KEY | keep-text KEY | to-png NAME [--force] | from-png NAME | compare DIR IMAGE... | downscale IMAGE OUT.png PIXEL [COLOURS] | icon OUT.png [KEY] | analyze IMAGE...")
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1)
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
