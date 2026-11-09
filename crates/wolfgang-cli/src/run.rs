//! Running a plan with the right kind of output, and the non-interactive mode.

use crate::flags::RunArgs;
use crate::ui::Ui;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use std::io::IsTerminal;
use std::path::Path;
use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use wolfgang_core::run::Event;
use wolfgang_core::{execute, Plan, Report, UserStore, Workspace};
use wolfgang_dialogue::context::{format_bytes, format_duration};

/// Set by Ctrl-C while converting; the engine stops before the next file.
pub static CANCEL: AtomicBool = AtomicBool::new(false);
static CONVERTING: AtomicBool = AtomicBool::new(false);

/// First Ctrl-C while converting: stop gracefully. Otherwise (or the second time): quit.
pub fn install_ctrlc() {
    let _ = ctrlc::set_handler(|| {
        if CONVERTING.load(Ordering::SeqCst) && !CANCEL.swap(true, Ordering::SeqCst) {
            eprintln!("\n  Okay, ich hör auf …");
            return;
        }
        let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
        std::process::exit(130);
    });
}

pub enum Output<'a> {
    Interactive { ui: &'a Ui, working: &'a [String] },
    Plain { ui: &'a Ui },
    Json,
}

pub fn convert(plan: &Plan, out: Output) -> wolfgang_core::Result<Report> {
    CANCEL.store(false, Ordering::SeqCst);
    CONVERTING.store(true, Ordering::SeqCst);
    let result = match out {
        Output::Json => {
            emit(json!({
                "event": "plan",
                "output_root": plan.output_root,
                "jobs": plan.jobs.len(),
                "skipped_known": plan.skipped_known.len(),
                "skipped_duplicates": plan.skipped_duplicates.len(),
                "problems": plan.problems.iter().map(|(p, e)| json!({"source": p, "error": e})).collect::<Vec<_>>(),
            }));
            let r = execute(
                plan,
                &|e| {
                    if let Event::FileDone(f) = e {
                        emit(json!({
                            "event": "file",
                            "source": f.rel,
                            "target": f.target,
                            "ok": f.outcome.is_ok(),
                            "bytes_in": f.bytes_in,
                            "bytes_out": f.outcome.as_ref().map(|d| d.bytes_out).unwrap_or(0),
                            "error": f.outcome.as_ref().err(),
                            "outputs": f.outcome.as_ref().map(|d| d.outputs.clone()).unwrap_or_default(),
                            "note": f.outcome.as_ref().ok().and_then(|d| d.note),
                        }));
                    }
                },
                &CANCEL,
            );
            if let Ok(r) = &r {
                emit(json!({
                    "event": "done",
                    "output_root": r.output_root,
                    "converted": r.converted,
                    "failed": r.failed.len(),
                    "bytes_in": r.bytes_in,
                    "bytes_out": r.bytes_out,
                    "duration_ms": r.duration.as_millis() as u64,
                    "cancelled": r.cancelled,
                }));
            }
            r
        }
        Output::Interactive { ui, working } => with_progress(plan, ui, working),
        Output::Plain { ui } => with_progress(plan, ui, &[]),
    };
    CONVERTING.store(false, Ordering::SeqCst);
    result
}

fn emit(v: serde_json::Value) {
    println!("{v}");
}

fn with_progress(plan: &Plan, ui: &Ui, working: &[String]) -> wolfgang_core::Result<Report> {
    for (p, e) in &plan.problems {
        ui.note(&format!("⚠ {}: {e}", p.display()));
    }
    if !std::io::stdout().is_terminal() {
        return execute(
            plan,
            &|e| {
                if let Event::FileDone(f) = e {
                    match &f.outcome {
                        Ok(_) => println!("✓ {} → {}", f.rel.display(), f.target.display()),
                        Err(msg) => println!("✗ {}: {msg}", f.rel.display()),
                    }
                }
            },
            &CANCEL,
        );
    }
    // The bar counts thousandths of files, so a long video moves it while it runs.
    let total = plan.jobs.len();
    let pb = ProgressBar::new(total as u64 * 1000);
    pb.set_style(
        ProgressStyle::with_template("  {spinner:.yellow} {wide_msg}\n  {bar:32.yellow/238} {prefix} · {elapsed}")
            .expect("valid template")
            .progress_chars("█▓░")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
    );
    pb.enable_steady_tick(Duration::from_millis(120));
    let line = |i: usize| working.get(i % working.len().max(1)).cloned().unwrap_or_else(|| "Bin dran …".into());
    pb.set_message(line(0));
    pb.set_prefix(format!("0/{total}"));
    let changed = Mutex::new((Instant::now(), 0usize));
    // Files done, and how far each running file is.
    let state = Mutex::new((0usize, HashMap::<std::path::PathBuf, f64>::new()));
    let report = execute(
        plan,
        &|e| {
            let mut st = state.lock().unwrap();
            match &e {
                Event::FileProgress { rel, fraction, .. } => {
                    st.1.insert(rel.clone(), *fraction);
                }
                Event::FileDone(f) => {
                    st.0 += 1;
                    st.1.remove(&f.rel);
                }
                _ => {}
            }
            let partial: f64 = st.1.values().sum();
            pb.set_position(st.0 as u64 * 1000 + (partial * 1000.0) as u64);
            pb.set_prefix(format!("{}/{total}", st.0));
            if let Event::FileDone(_) = e {
                let mut c = changed.lock().unwrap();
                if c.0.elapsed() > Duration::from_secs(4) {
                    *c = (Instant::now(), c.1 + 1);
                    pb.set_message(line(c.1));
                }
            }
        },
        &CANCEL,
    );
    pb.finish_and_clear();
    report
}

/// No questions: defaults, preset and flags decide.
pub fn non_interactive(ui: &Ui, ws: Workspace, store: &mut UserStore, store_path: &Path, args: &RunArgs) -> ExitCode {
    let s = ws.summary();
    if s.media_tools_missing {
        ui.note("Für Videos und Audio fehlt mir mein Werkzeug (ffmpeg), die lass ich liegen. Installier mich am besten neu.");
    }
    let (settings, rerun) = match args.settings(store, s.piles()) {
        Ok(v) => v,
        Err(e) => {
            ui.error(&e);
            return ExitCode::from(2);
        }
    };
    let plan = match ws.plan(&settings, rerun) {
        Ok(p) => p,
        Err(e) => {
            ui.error(&e.to_string());
            return ExitCode::from(1);
        }
    };
    if !args.json {
        if plan.jobs.is_empty() {
            let why = if plan.skipped_known.is_empty() { "Nix dabei, was ich umwandeln kann." } else { "Ist schon alles fertig. (Mit --all mach ich's nochmal.)" };
            ui.say(&[why.into()], None);
            return ExitCode::SUCCESS;
        }
        let mut msg = format!("Mach ich: {} {} → „{}“.", plan.jobs.len(), if plan.jobs.len() == 1 { "Datei" } else { "Dateien" }, folder_name(&plan));
        if !plan.skipped_known.is_empty() {
            msg.push_str(&format!(" {} kenn ich schon, die lass ich aus.", plan.skipped_known.len()));
        }
        ui.say(&[msg], None);
    }
    let out = if args.json { Output::Json } else { Output::Plain { ui } };
    let report = match convert(&plan, out) {
        Ok(r) => r,
        Err(e) => {
            ui.error(&e.to_string());
            return ExitCode::from(1);
        }
    };
    if !args.json {
        let saved = (report.bytes_out * 100).checked_div(report.bytes_in).map_or(0, |k| 100u64.saturating_sub(k));
        let mut first = format!(
            "{} {} in {}",
            if report.cancelled { "Abgebrochen nach" } else { "Fertig!" },
            if report.converted == 1 { "1 Datei".to_string() } else { format!("{} Dateien", report.converted) },
            format_duration(report.duration),
        );
        if report.bytes_in > 0 {
            first.push_str(&format!(": {} → {} ({saved} % gespart)", format_bytes(report.bytes_in), format_bytes(report.bytes_out)));
        }
        first.push('.');
        if report.pdf_pictures > 0 {
            first.push_str(&format!(" Aus den PDFs: {} Bilder.", report.pdf_pictures));
        }
        let mut lines = vec![first];
        for (p, note) in &report.notes {
            lines.push(format!("ℹ {}: {note}", p.display()));
        }
        for (p, e) in &report.failed {
            lines.push(format!("✗ {}: {e}", p.display()));
        }
        ui.say(&lines, None);
    }
    if report.converted > 0 {
        store.remember_run(&settings);
        let _ = store.save(store_path);
    }
    if report.failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `Urlaub/wolfgang_output`
fn folder_name(plan: &Plan) -> String {
    let parts: Vec<String> = plan.output_root.iter().rev().take(2).map(|p| p.to_string_lossy().into_owned()).collect();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}
