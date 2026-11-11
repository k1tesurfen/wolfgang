//! `wolfgang`: the command line face of Wolfgang.
//!
//! Exit codes: 0 ok, 1 some files failed (or another error), 2 usage error, 130 aborted.

mod art;
mod flags;
mod legacy;
mod manage;
mod run;
mod tui;
mod ui;

use clap::{CommandFactory, FromArgMatches};
use flags::{Cli, Command, OnlyArg};
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;
use ui::Ui;
use wolfgang_core::{FileKind, UserStore, Workspace};

fn main() -> ExitCode {
    let cli = parse_args();
    let ui = Ui::new();
    run::install_ctrlc();

    let store_path = UserStore::default_path();
    let mut store = match UserStore::load(&store_path) {
        Ok(l) => {
            if let Some(backup) = l.recovered_from {
                ui.note(&format!("Meine Einstellungen waren durcheinander, ich hab neu angefangen (alte Datei: {}).", backup.display()));
            }
            l.store
        }
        Err(e) => {
            ui.error(&format!("Kann meine Einstellungen nicht lesen: {e}"));
            return ExitCode::from(1);
        }
    };

    match cli.command {
        Some(Command::Presets { action }) => manage::presets(&ui, &mut store, &store_path, action),
        Some(Command::Settings { action }) => manage::settings(&ui, &mut store, &store_path, action),
        Some(Command::Transcription { action }) => manage::transcription(&ui, action),
        Some(Command::LegacyReset { path, yes }) => legacy_reset(&ui, &path, yes),
        None if cli.run.reset => {
            let dir = cli.run.paths.first().cloned().unwrap_or_else(|| ".".into());
            legacy_reset(&ui, &dir, cli.run.yes)
        }
        None => convert(&ui, &mut store, &store_path, cli.run),
    }
}

/// The arguments, with the help texts naming this build's character (Ully, Burgunde, …).
fn parse_args() -> Cli {
    let name = &wolfgang_core::character::current().name;
    let mut cmd = Cli::command();
    let named = |t: Option<&clap::builder::StyledStr>| t.map(|t| t.to_string().replace("Wolfgang", name));
    if let Some(about) = named(cmd.get_about()) {
        cmd = cmd.about(about);
    }
    if let Some(long) = named(cmd.get_long_about()) {
        cmd = cmd.long_about(long);
    }
    Cli::from_arg_matches(&cmd.get_matches()).unwrap_or_else(|e| e.exit())
}

fn convert(ui: &Ui, store: &mut UserStore, store_path: &Path, mut args: flags::RunArgs) -> ExitCode {
    let interactive = !args.yes && !args.json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if interactive {
        tui::setup_prompts();
    }

    // Validate flags before scanning anything.
    let fixed = match args.fixed(store) {
        Ok(f) => f,
        Err(e) => {
            ui.error(&e);
            return ExitCode::from(2);
        }
    };
    if args.wants_transcripts() && !wolfgang_core::transcribe::installed() {
        ui.error("Fürs Aufschreiben fehlt mir noch das Zusatzmodul (≈ 575 MB). Installier es mit: wolfgang transcription install");
        return ExitCode::from(2);
    }

    if args.paths.is_empty() {
        if !interactive {
            ui.error("Sag mir, welchen Ordner: wolfgang ORDNER (oder wolfgang --help)");
            return ExitCode::from(2);
        }
        match tui::ask_for_paths(ui) {
            Some(p) => args.paths = p,
            None => return ExitCode::SUCCESS,
        }
    }
    if let Some(missing) = args.paths.iter().find(|p| !p.exists()) {
        ui.error(&format!("„{}“ find ich nicht.", missing.display()));
        return ExitCode::from(2);
    }

    // Hashing big folders can take a moment; a spinner that vanishes once Wolfgang talks.
    let looking = interactive.then(|| {
        let pb = indicatif::ProgressBar::new_spinner().with_style(
            indicatif::ProgressStyle::with_template("  {spinner:.yellow} {msg}").expect("valid template").tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        pb.set_message(format!("{} guckt sich das an …", wolfgang_core::character::current().name));
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        pb
    });
    let opened = Workspace::open(&args.paths);
    if let Some(pb) = looking {
        pb.finish_and_clear();
    }
    let mut ws = match opened {
        Ok(ws) => ws,
        Err(e) => {
            ui.error(&e.to_string());
            return ExitCode::from(1);
        }
    };
    match args.only {
        Some(OnlyArg::Images) => ws.keep_only(FileKind::Image),
        Some(OnlyArg::Videos) => ws.keep_only(FileKind::Video),
        Some(OnlyArg::Audio) => ws.keep_only(FileKind::Audio),
        Some(OnlyArg::Pdfs) => ws.keep_only(FileKind::Pdf),
        Some(OnlyArg::Tables) => ws.keep_only(FileKind::Table),
        Some(OnlyArg::Fonts) => ws.keep_only(FileKind::Font),
        None => {}
    }

    if interactive {
        tui::converse(ui, ws, store, store_path, fixed)
    } else {
        run::non_interactive(ui, ws, store, store_path, &args)
    }
}

fn legacy_reset(ui: &Ui, dir: &Path, yes: bool) -> ExitCode {
    let plan = match legacy::plan(dir) {
        Ok(Some(p)) => p,
        Ok(None) => {
            ui.say(&[format!("In „{}“ hab ich kein altes Logbuch gefunden. Da gibt's nix zurückzudrehen.", dir.display())], None);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            ui.error(&e);
            return ExitCode::from(1);
        }
    };
    let rel = |p: &Path| p.strip_prefix(dir).unwrap_or(p).display().to_string();
    for (from, to) in &plan.renames {
        println!("  {} → {}", rel(from), to.file_name().unwrap_or_default().to_string_lossy());
    }
    for (from, to) in &plan.blocked {
        ui.note(&format!("übersprungen: {} („{}“ gibt's schon)", rel(from), to.file_name().unwrap_or_default().to_string_lossy()));
    }
    if !plan.missing.is_empty() {
        ui.note(&format!("{} Dateien aus dem Logbuch gibt's nicht mehr, die lass ich.", plan.missing.len()));
    }
    if plan.renames.is_empty() {
        ui.say(&["Nix umzubenennen.".into()], None);
        return ExitCode::SUCCESS;
    }
    let go = yes
        || (std::io::stdin().is_terminal()
            && inquire::Confirm::new(&format!("{} Originaldateien zurück umbenennen?", plan.renames.len())).with_default(true).prompt().unwrap_or(false));
    if !go {
        ui.say(&["Hab nix angefasst. (Mit --yes mach ich's ohne Nachfrage.)".into()], None);
        return ExitCode::SUCCESS;
    }
    let errors = legacy::apply(dir, &plan);
    for (p, e) in &errors {
        ui.error(&format!("{}: {e}", rel(p)));
    }
    ui.say(
        &[format!("{} Dateien heißen wieder wie früher. Die alten *-wolfgang-Ordner hab ich nicht angefasst.", plan.renames.len() - errors.len())],
        None,
    );
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
