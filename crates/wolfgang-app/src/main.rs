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
use tauri_plugin_updater::{Update, UpdaterExt};
use wolfgang_core::run::Event;
use wolfgang_core::store::Defaults;
use wolfgang_core::{transcribe, UserStore};

struct AppState {
    office: Mutex<Office>,
    cancel: Arc<AtomicBool>,
    /// Files dropped on the Dock icon, kept until the UI picks them up
    /// (they can arrive before the UI is ready, e.g. when they launch the app).
    pending: Mutex<Vec<String>>,
    /// A newer Wolfgang found on GitHub, waiting for a yes.
    update: Mutex<Option<Update>>,
    /// The add-on for „Nur aufschreiben“ is being downloaded …
    addon_installing: Arc<AtomicBool>,
    /// … and should stop.
    addon_cancel: Arc<AtomicBool>,
}

type Res<T> = Result<T, String>;

fn lock<'a>(state: &'a State<'_, AppState>) -> std::sync::MutexGuard<'a, Office> {
    state.office.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
async fn drop_files(state: State<'_, AppState>, paths: Vec<String>) -> Res<View> {
    eprintln!("wolfgang-app: {} item(s) on the desk", paths.len());
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
    /// Overall progress 0–1, including how far running videos are.
    fraction: f64,
    /// `Some` when a file just finished (stamp!), `None` for progress within a file.
    ok: Option<bool>,
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
        // Files done, and how far each running file is.
        let state = std::sync::Mutex::new((0usize, std::collections::HashMap::<std::path::PathBuf, f64>::new()));
        let report = wolfgang_core::execute(
            &plan,
            &|e| {
                let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                let (current, ok) = match &e {
                    Event::FileProgress { rel, fraction, .. } => {
                        st.1.insert(rel.clone(), *fraction);
                        (rel, None)
                    }
                    Event::FileDone(f) => {
                        st.0 += 1;
                        st.1.remove(&f.rel);
                        (&f.rel, Some(f.outcome.is_ok()))
                    }
                    _ => return,
                };
                let fraction = (st.0 as f64 + st.1.values().sum::<f64>()) / total.max(1) as f64;
                let _ = app.emit("progress", Progress { done: st.0, total, current: current.display().to_string(), fraction, ok });
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

/// The Finder dialog (desk click, ⌘O): files and folders, several at once.
/// Returns the chosen paths; empty if the user cancelled.
#[tauri::command]
async fn pick_files(app: AppHandle) -> Res<Vec<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(open_panel());
    })
    .map_err(|e| e.to_string())?;
    // The dialog is modal on the main thread; wait for it off the async workers.
    tauri::async_runtime::spawn_blocking(move || rx.recv()).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
fn open_panel() -> Vec<String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::NSString;
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(true);
    panel.setPrompt(Some(&NSString::from_str("Auf den Tisch")));
    panel.setMessage(Some(&NSString::from_str("Was soll Wolfgang sich angucken? Dateien oder ganze Ordner, gern mehrere.")));
    if panel.runModal() != NSModalResponseOK {
        return Vec::new();
    }
    panel.URLs().iter().filter_map(|url| url.path()).map(|p| p.to_string()).collect()
}

#[cfg(not(target_os = "macos"))]
fn open_panel() -> Vec<String> {
    Vec::new()
}

/// Files that arrived via the Dock icon since the last call.
#[tauri::command]
fn take_pending(state: State<AppState>) -> Vec<String> {
    std::mem::take(&mut *state.pending.lock().unwrap_or_else(|e| e.into_inner()))
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

#[derive(Clone, Serialize)]
struct UpdateInfo {
    version: String,
}

/// Looks on GitHub for a newer Wolfgang. Offline, or in a dev build: nothing.
#[tauri::command]
async fn check_update(app: AppHandle, state: State<'_, AppState>) -> Res<Option<UpdateInfo>> {
    if cfg!(debug_assertions) || std::env::var_os("WOLFGANG_NO_UPDATE").is_some() {
        return Ok(None);
    }
    let found = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let info = found.as_ref().map(|u| UpdateInfo { version: u.version.clone() });
    if let Some(i) = &info {
        eprintln!("wolfgang-app: update available: {}", i.version);
    }
    *state.update.lock().unwrap_or_else(|e| e.into_inner()) = found;
    Ok(info)
}

#[derive(Clone, Serialize)]
struct Download {
    done: usize,
    total: Option<u64>,
}

/// Downloads the update found by `check_update` (progress as `update-progress`
/// events), swaps the app and starts the new one.
#[tauri::command]
async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Res<()> {
    let update = state.update.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or("Da ist gerade kein neuer Wolfgang.")?;
    let mut done = 0;
    update
        .download_and_install(
            |chunk, total| {
                done += chunk;
                let _ = app.emit("update-progress", Download { done, total });
            },
            || {},
        )
        .await
        .map_err(|e| format!("Das Auffrischen hat nicht geklappt: {e}"))?;
    eprintln!("wolfgang-app: updated to {}, restarting", update.version);
    app.restart()
}

/// The add-on for „Nur aufschreiben“, for the settings panel.
#[derive(Clone, Serialize)]
struct Addon {
    installed: bool,
    installing: bool,
    /// Bytes to download.
    size: u64,
}

fn addon(state: &AppState) -> Addon {
    Addon { installed: transcribe::installed(), installing: state.addon_installing.load(Ordering::SeqCst), size: transcribe::install_size() }
}

#[tauri::command]
fn transcription(state: State<AppState>) -> Addon {
    addon(&state)
}

/// Downloads the add-on in the background: `transcription-progress` events while
/// it runs, then `transcription-done` with the new state or an error message.
#[tauri::command]
fn install_transcription(app: AppHandle, state: State<AppState>) -> Addon {
    if !state.addon_installing.swap(true, Ordering::SeqCst) {
        state.addon_cancel.store(false, Ordering::SeqCst);
        let cancel = state.addon_cancel.clone();
        std::thread::spawn(move || {
            let result = transcribe::install(&|done, total| drop(app.emit("transcription-progress", Download { done: done as usize, total: Some(total) })), &cancel);
            let state = app.state::<AppState>();
            state.addon_installing.store(false, Ordering::SeqCst);
            eprintln!("wolfgang-app: add-on install: {}", result.as_ref().map_or_else(|e| e.to_string(), |_| "ok".into()));
            let done: Result<Addon, String> = match result {
                Ok(()) | Err(wolfgang_core::Error::Cancelled) => Ok(addon(&state)),
                Err(e) => Err(e.to_string()),
            };
            let _ = app.emit("transcription-done", done);
        });
    }
    addon(&state)
}

#[tauri::command]
fn cancel_transcription_install(state: State<AppState>) {
    state.addon_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn remove_transcription(state: State<AppState>) -> Res<Addon> {
    if lock(&state).running {
        return Err("Moment, ich bin gerade dran. Danach kannst du's entfernen.".into());
    }
    transcribe::remove().map_err(|e| e.to_string())?;
    Ok(addon(&state))
}

/// Scene size in points; the window is always an exact whole multiple of it.
const SCENE: (f64, f64) = (320.0, 180.0);
/// 1280×720: comfortable on Full HD and on Retina screens alike.
const DEFAULT_SCALE: u32 = 4;
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

/// The saved size (or the default), but never bigger than the screen allows.
fn start_scale(window: &tauri::WebviewWindow, saved: Option<u32>) -> u32 {
    saved.unwrap_or(DEFAULT_SCALE).clamp(1, max_scale(window))
}

/// ⌘+ / ⌘− grow or shrink the office by one whole step and remember it;
/// ⌘0 goes back to the default size.
#[tauri::command]
fn zoom(window: tauri::WebviewWindow, state: State<AppState>, delta: i32) -> u32 {
    let (scale, remember) = if delta == 0 {
        (start_scale(&window, None), None)
    } else {
        let s = (current_scale(&window) as i32 + delta).clamp(1, max_scale(&window) as i32) as u32;
        (s, Some(s))
    };
    set_scale(&window, scale);
    let _ = lock(&state).set_window_scale(remember);
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
    match std::env::var("WOLFGANG_SELFTEST").as_deref() {
        // Checks for an update and installs it without asking (tests the whole round trip).
        Ok("update") => {
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let result = match check_update(app.clone(), state.clone()).await {
                    Ok(Some(_)) => install_update(app.clone(), state).await,
                    Ok(None) => Err("no update found".into()),
                    Err(e) => Err(e),
                };
                if let Err(e) = result {
                    eprintln!("wolfgang-app: update self-test failed: {e}");
                    app.exit(1);
                }
            });
        }
        Ok(_) => app.exit(if errors.is_empty() && scale > 0.0 { 0 } else { 1 }),
        Err(_) => {}
    }
}

fn main() {
    let store_path = UserStore::default_path();
    let store = UserStore::load(&store_path).map(|l| l.store).unwrap_or_default();
    let script = wolfgang_dialogue::load().unwrap_or_else(|_| wolfgang_dialogue::builtin());
    let state = AppState {
        office: Mutex::new(Office::new(store_path, store, script)),
        cancel: Arc::new(AtomicBool::new(false)),
        pending: Mutex::new(Vec::new()),
        update: Mutex::new(None),
        addon_installing: Arc::new(AtomicBool::new(false)),
        addon_cancel: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            zoom,
            take_pending,
            pick_files,
            check_update,
            install_update,
            transcription,
            install_transcription,
            cancel_transcription_install,
            remove_transcription
        ])
        .setup(|app| {
            // Exactly the office, at the largest whole scale that fits this screen.
            if let Some(window) = app.get_webview_window("main") {
                let saved = app.state::<AppState>().office.lock().map(|o| o.store.defaults.window_scale).unwrap_or(None);
                set_scale(&window, start_scale(&window, saved));
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
                _app.state::<AppState>().pending.lock().unwrap_or_else(|e| e.into_inner()).extend(paths);
                let _ = _app.emit("files-opened", ());
            }
        });
}
