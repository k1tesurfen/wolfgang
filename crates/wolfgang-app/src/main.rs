//! Wolfgang's office: the macOS app. The UI lives in `ui/`; this wires it to
//! the engine through a handful of commands and events.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod session;

use serde::Serialize;
use session::{About, Office, PresetView, View};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use wolfgang_core::run::Event;
use wolfgang_core::store::Defaults;
use wolfgang_core::UserStore;

struct AppState {
    office: Mutex<Office>,
    cancel: Arc<AtomicBool>,
}

type Res<T> = Result<T, String>;

fn lock<'a>(state: &'a State<'_, AppState>) -> std::sync::MutexGuard<'a, Office> {
    state.office.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
async fn drop_files(state: State<'_, AppState>, paths: Vec<String>) -> Res<View> {
    lock(&state).drop_files(paths.into_iter().map(PathBuf::from).collect())
}

#[tauri::command]
fn choose(state: State<AppState>, index: usize) -> Res<View> {
    lock(&state).choose(index)
}

#[tauri::command]
fn type_answer(state: State<AppState>, text: String) -> Res<View> {
    lock(&state).type_answer(&text)
}

#[tauri::command]
fn back(state: State<AppState>) -> Res<View> {
    lock(&state).back()
}

#[derive(Clone, Serialize)]
struct Progress {
    done: usize,
    total: usize,
    current: String,
    ok: bool,
}

/// Starts converting in the background. Progress arrives as `progress` events,
/// the end as a `finished` event carrying the next view.
#[tauri::command]
fn start(app: AppHandle, state: State<AppState>) -> Res<()> {
    let plan = lock(&state).begin_run()?;
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    std::thread::spawn(move || {
        let total = plan.jobs.len();
        let done = std::sync::atomic::AtomicUsize::new(0);
        let report = wolfgang_core::execute(
            &plan,
            &|e| {
                if let Event::FileDone(f) = e {
                    let done = done.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = app.emit("progress", Progress { done, total, current: f.rel.display().to_string(), ok: f.outcome.is_ok() });
                }
            },
            &cancel,
        );
        let state = app.state::<AppState>();
        let mut office = state.office.lock().unwrap_or_else(|e| e.into_inner());
        let view = match report {
            Ok(r) => office.finished(&r),
            Err(e) => {
                office.running = false;
                Err(e.to_string())
            }
        };
        drop(office);
        match view {
            Ok(v) => app.emit("finished", v),
            Err(e) => app.emit("failed", e),
        }
        .ok();
    });
    Ok(())
}

#[tauri::command]
fn cancel(state: State<AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn working_line(state: State<AppState>) -> String {
    lock(&state).working_line()
}

#[tauri::command]
fn idle_line(state: State<AppState>) -> String {
    lock(&state).idle_line()
}

#[tauri::command]
fn settings(state: State<AppState>) -> Defaults {
    lock(&state).defaults()
}

#[tauri::command]
fn set_settings(state: State<AppState>, defaults: Defaults) -> Res<Defaults> {
    lock(&state).set_defaults(defaults)
}

#[tauri::command]
fn presets(state: State<AppState>) -> Vec<PresetView> {
    lock(&state).presets()
}

#[tauri::command]
fn remove_preset(state: State<AppState>, name: String) -> Res<Vec<PresetView>> {
    lock(&state).remove_preset(&name)
}

#[tauri::command]
fn reveal(path: String) -> Res<()> {
    std::process::Command::new("open").arg("-R").arg(&path).status().map(|_| ()).map_err(|e| e.to_string())
}

/// Where the `wolfgang` command line tool lives inside the app (bundled next to
/// this executable), and where it gets linked to.
fn cli_paths() -> (Option<PathBuf>, PathBuf) {
    let bundled = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("wolfgang"))).filter(|p| p.exists());
    (bundled, PathBuf::from("/usr/local/bin/wolfgang"))
}

#[tauri::command]
fn about() -> About {
    let (bundled, link) = cli_paths();
    let installed = match (&bundled, std::fs::canonicalize(&link)) {
        (Some(b), Ok(target)) => std::fs::canonicalize(b).is_ok_and(|b| b == target),
        _ => false,
    };
    About { version: env!("CARGO_PKG_VERSION").into(), cli_installed: installed }
}

/// Links the bundled CLI to /usr/local/bin (macOS asks for the admin password).
#[tauri::command]
fn install_cli() -> Res<About> {
    let (bundled, link) = cli_paths();
    let bundled = bundled.ok_or("In dieser Version ist die Kommandozeile nicht mit eingepackt.")?;
    let quote = |p: &std::path::Path| format!("'{}'", p.display().to_string().replace('\'', r"'\''"));
    let script = format!(
        "do shell script \"mkdir -p /usr/local/bin && ln -sf {} {}\" with administrator privileges",
        quote(&bundled).replace('"', "\\\""),
        quote(&link).replace('"', "\\\"")
    );
    let status = std::process::Command::new("osascript").arg("-e").arg(script).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("Hat nicht geklappt (oder du hast abgebrochen).".into());
    }
    Ok(about())
}

/// Scene size in points; the window is always an exact whole multiple of it.
const SCENE: (f64, f64) = (320.0, 180.0);
/// Leave room for the menu bar and the Dock.
const SCREEN_USE: f64 = 0.75;

/// The largest whole scale at which the scene fits on the window's screen.
fn max_scale(window: &tauri::WebviewWindow) -> u32 {
    let Ok(Some(m)) = window.current_monitor().or_else(|_| window.primary_monitor()) else { return 4 };
    let size = m.size().to_logical::<f64>(m.scale_factor());
    let fit = (size.width * SCREEN_USE / SCENE.0).min(size.height * SCREEN_USE / SCENE.1);
    (fit.floor() as u32).max(1)
}

fn current_scale(window: &tauri::WebviewWindow) -> u32 {
    let sf = window.scale_factor().unwrap_or(1.0);
    window.inner_size().map(|s| (s.to_logical::<f64>(sf).width / SCENE.0).round() as u32).unwrap_or(4).max(1)
}

fn set_scale(window: &tauri::WebviewWindow, scale: u32) {
    let size = tauri::LogicalSize::new(SCENE.0 * scale as f64, SCENE.1 * scale as f64);
    let _ = window.set_size(size);
}

/// ⌘+ / ⌘− grow or shrink the office by one whole step, ⌘0 picks the best fit.
#[tauri::command]
fn zoom(window: tauri::WebviewWindow, delta: i32) -> u32 {
    let max = max_scale(&window);
    let scale = if delta == 0 { max } else { (current_scale(&window) as i32 + delta).clamp(1, max as i32) as u32 };
    set_scale(&window, scale);
    scale
}

/// The UI reports in after starting: errors go to the log, and with
/// `WOLFGANG_SELFTEST=1` the app quits right away (exit code 1 on errors).
#[tauri::command]
fn frontend_ready(app: AppHandle, errors: Vec<String>, scale: f64) {
    eprintln!("wolfgang-app: UI ready (scale {scale}), {} error(s)", errors.len());
    for e in &errors {
        eprintln!("  ui error: {e}");
    }
    if std::env::var_os("WOLFGANG_SELFTEST").is_some() {
        app.exit(if errors.is_empty() && scale > 0.0 { 0 } else { 1 });
    }
}

fn main() {
    let store_path = UserStore::default_path();
    let store = UserStore::load(&store_path).map(|l| l.store).unwrap_or_default();
    let script = wolfgang_dialogue::load().unwrap_or_else(|_| wolfgang_dialogue::builtin());
    let state = AppState { office: Mutex::new(Office::new(store_path, store, script)), cancel: Arc::new(AtomicBool::new(false)) };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            drop_files,
            choose,
            type_answer,
            back,
            start,
            cancel,
            working_line,
            idle_line,
            settings,
            set_settings,
            presets,
            remove_preset,
            reveal,
            about,
            install_cli,
            frontend_ready,
            zoom
        ])
        .setup(|app| {
            // Exactly the office, at the largest whole scale that fits this screen.
            if let Some(window) = app.get_webview_window("main") {
                set_scale(&window, max_scale(&window));
                let _ = window.center();
                let _ = window.show();
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Wolfgang's office won't open")
        .run(|_app, _event| {
            // Files dropped on the Dock icon (or opened with Wolfgang).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &_event {
                let paths: Vec<String> = urls.iter().filter_map(|u| u.to_file_path().ok()).map(|p| p.display().to_string()).collect();
                let _ = _app.emit("files-opened", paths);
            }
        });
}
