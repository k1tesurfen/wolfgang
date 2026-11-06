//! `wolfgang presets` and `wolfgang settings`.

use crate::flags::{PresetAction, SettingKey, SettingsAction};
use crate::ui::Ui;
use std::path::Path;
use std::process::ExitCode;
use wolfgang_core::naming::Separator;
use wolfgang_core::settings::MAX_SIDE_RANGE;
use wolfgang_core::{ImageFormat, UserStore, WebFormat};
use wolfgang_dialogue::context::describe_images;

pub fn presets(ui: &Ui, store: &mut UserStore, path: &Path, action: Option<PresetAction>) -> ExitCode {
    match action.unwrap_or(PresetAction::List) {
        PresetAction::List => {
            if store.presets.is_empty() {
                ui.say(&["Noch keine Vorlagen. Nach einer Runde kann ich mir die Einstellungen merken.".into()], None);
            }
            for p in &store.presets {
                let what = p.settings.images.as_ref().map(describe_images).unwrap_or_else(|| "nur Schriften".into());
                println!("  {} – {what}", p.name);
            }
            ExitCode::SUCCESS
        }
        PresetAction::Remove { name } => {
            if !store.remove_preset(&name) {
                ui.error(&format!("Vorlage „{name}“ kenn ich nicht."));
                return ExitCode::from(2);
            }
            save(ui, store, path);
            ui.say(&[format!("„{name}“ ist raus aus meinem Notizbuch.")], None);
            ExitCode::SUCCESS
        }
    }
}

pub fn settings(ui: &Ui, store: &mut UserStore, path: &Path, action: Option<SettingsAction>) -> ExitCode {
    match action.unwrap_or(SettingsAction::Show) {
        SettingsAction::Show => {
            let d = &store.defaults;
            println!("  web-format  {}", ImageFormat::from(d.web_format).label());
            println!("  size        {} px", d.max_side);
            println!("  quality     {}", d.quality);
            println!("  hq-format   {}", d.hq_format.label());
            println!("  separator   {:?}", d.separator);
            println!("  sound       {}", if d.sound { "an" } else { "aus" });
            if let Some(last) = store.last_run.as_ref().and_then(|r| r.images.as_ref()) {
                println!("\n  Letztes Mal: {}", describe_images(last));
            }
            ui.note(&format!("Gespeichert in {}", path.display()));
            ExitCode::SUCCESS
        }
        SettingsAction::Set { key, value } => match set(store, key, &value) {
            Ok(()) => {
                save(ui, store, path);
                ui.say(&["Notiert.".into()], None);
                ExitCode::SUCCESS
            }
            Err(e) => {
                ui.error(&e);
                ExitCode::from(2)
            }
        },
    }
}

fn set(store: &mut UserStore, key: SettingKey, value: &str) -> Result<(), String> {
    let d = &mut store.defaults;
    let v = value.trim().to_lowercase();
    match key {
        SettingKey::WebFormat => {
            d.web_format = match v.as_str() {
                "webp" => WebFormat::Webp,
                "avif" => WebFormat::Avif,
                _ => return Err("Fürs Web gibt's webp oder avif.".into()),
            }
        }
        SettingKey::HqFormat => {
            d.hq_format = match v.as_str() {
                "jpg" | "jpeg" => ImageFormat::Jpg,
                "webp" => ImageFormat::Webp,
                "avif" => ImageFormat::Avif,
                "png" => ImageFormat::Png,
                _ => return Err("Hochwertig gibt's als jpg, webp, avif oder png.".into()),
            }
        }
        SettingKey::Size => {
            let n: u32 = v.trim_end_matches("px").trim().parse().map_err(|_| "Die Größe ist eine Zahl in Pixeln.")?;
            if !MAX_SIDE_RANGE.contains(&n) {
                return Err(format!("Zwischen {} und {} Pixel, bitte.", MAX_SIDE_RANGE.start(), MAX_SIDE_RANGE.end()));
            }
            d.max_side = n;
        }
        SettingKey::Quality => {
            let n: u8 = v.parse().ok().filter(|n| (1..=100).contains(n)).ok_or("Qualität geht von 1 bis 100.")?;
            d.quality = n;
        }
        SettingKey::Separator => {
            d.separator = match v.as_str() {
                "dash" => Separator::Dash,
                "underscore" => Separator::Underscore,
                "caps" => Separator::Caps,
                _ => return Err("dash, underscore oder caps.".into()),
            }
        }
        SettingKey::Sound => {
            d.sound = match v.as_str() {
                "an" | "on" | "ja" | "true" => true,
                "aus" | "off" | "nein" | "false" => false,
                _ => return Err("an oder aus.".into()),
            }
        }
    }
    Ok(())
}

fn save(ui: &Ui, store: &UserStore, path: &Path) {
    if let Err(e) = store.save(path) {
        ui.error(&format!("Konnte nicht speichern: {e}"));
    }
}
