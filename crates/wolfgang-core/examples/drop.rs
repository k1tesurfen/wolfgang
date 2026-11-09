//! Developer stand-in for the CLI (milestone 4): drop paths on Wolfgang with default settings.
//!
//!   cargo run --release -p wolfgang-core --example drop -- [--hq FORMAT | --avif] [--all] PATH...

use std::sync::atomic::AtomicBool;
use wolfgang_core::*;

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take = |flag: &str| args.iter().position(|a| a == flag).map(|i| args.remove(i));
    let avif = take("--avif").is_some();
    let all = take("--all").is_some();
    let hq = take("--hq").map(|_| ());
    let hq_format = hq.map(|_| match args.remove(0).as_str() {
        "jpg" => ImageFormat::Jpg,
        "png" => ImageFormat::Png,
        "avif" => ImageFormat::Avif,
        _ => ImageFormat::Webp,
    });
    let items: Vec<_> = args.iter().map(Into::into).collect();

    let ws = Workspace::open(&items)?;
    let s = ws.summary();
    println!("Ablage: {} Bilder ({} neu), {} Schriften ({} neu), {} unbekannt, {} doppelt", s.images.total, s.images.fresh(), s.fonts.total, s.fonts.fresh(), s.unknown.len(), s.duplicates);
    println!("Ziel:   {}", ws.output_root.display());

    let images = match hq_format {
        Some(format) => ImageSettings::HighQuality { format },
        None => ImageSettings::Web(WebSettings {
            format: if avif { WebFormat::Avif } else { WebFormat::Webp },
            naming: Naming { use_keyword_files: true, ..Default::default() },
            ..Default::default()
        }),
    };
    let settings = RunSettings { images: Some(images), fonts: Some(FontSettings {}), ..Default::default() };
    let plan = ws.plan(&settings, if all { RerunPolicy::All } else { RerunPolicy::OnlyNew })?;
    for (p, why) in &plan.problems {
        println!("  ⚠ {}: {why}", p.display());
    }

    let report = execute(
        &plan,
        &|e| {
            if let run::Event::FileDone(r) = e {
                match &r.outcome {
                    Ok(d) => println!("  ✓ {} → {}  ({} KB → {} KB)", r.rel.display(), r.target.display(), r.bytes_in / 1024, d.bytes_out / 1024),
                    Err(msg) => println!("  ✗ {}: {msg}", r.rel.display()),
                }
            }
        },
        &AtomicBool::new(false),
    )?;
    println!(
        "Fertig: {} Dateien, {:.1} MB → {:.1} MB in {:.1?}{}",
        report.converted,
        report.bytes_in as f64 / 1e6,
        report.bytes_out as f64 / 1e6,
        report.duration,
        if report.failed.is_empty() { String::new() } else { format!(", {} Fehler", report.failed.len()) }
    );
    for (p, note) in &report.notes {
        println!("  ℹ {}: {note}", p.display());
    }
    Ok(())
}
