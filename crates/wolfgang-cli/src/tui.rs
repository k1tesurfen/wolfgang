//! The interactive conversation in the terminal.

use crate::run::{convert, Output};
use crate::ui::Ui;
use inquire::error::InquireError;
use inquire::ui::{Color, RenderConfig, StyleSheet, Styled};
use inquire::{Select, Text};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use wolfgang_core::{UserStore, Workspace};
use wolfgang_dialogue::script::{InputKind, Value};
use wolfgang_dialogue::{Context, Conversation, Step};

const TYPE_OWN: &str = "✎ Selbst eintippen …";
const GO_BACK: &str = "← Zurück";

/// Ctrl-C inside a prompt.
struct Quit;

pub fn setup_prompts() {
    let mut cfg = RenderConfig::default_colored();
    cfg.prompt_prefix = Styled::new("›").with_fg(Color::DarkYellow);
    cfg.answered_prompt_prefix = Styled::new("›").with_fg(Color::DarkGrey);
    cfg.highlighted_option_prefix = Styled::new("▶").with_fg(Color::DarkYellow);
    cfg.selected_option = Some(StyleSheet::new().with_fg(Color::DarkYellow));
    cfg.answer = StyleSheet::new().with_fg(Color::Grey);
    inquire::set_global_render_config(cfg);
}

/// Asks for paths when none were given. Folders dragged into the terminal
/// arrive as text (`/Users/x/Mein\ Ordner`), so that's what we parse.
pub fn ask_for_paths(ui: &Ui) -> Option<Vec<PathBuf>> {
    let script = wolfgang_dialogue::load().ok()?;
    ui.say(&[Conversation::idle_line(&script, &mut fastrand::Rng::new())], Some(wolfgang_dialogue::Mood::Happy));
    let here = std::env::current_dir().ok()?;
    let here_name = here.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| here.display().to_string());
    loop {
        let answer = Text::new("Du")
            .with_placeholder("Ordner hier reinziehen")
            .with_help_message(&format!("Ordner oder Dateien ins Fenster ziehen und Enter · nur Enter = aktueller Ordner „{here_name}“ · Esc = tschüss"))
            .prompt();
        let raw = match answer {
            Ok(s) => s,
            Err(_) => return None,
        };
        let paths = if raw.trim().is_empty() { vec![here.clone()] } else { parse_dropped(&raw) };
        let missing: Vec<_> = paths.iter().filter(|p| !p.exists()).collect();
        if missing.is_empty() && !paths.is_empty() {
            return Some(paths);
        }
        ui.say(&[format!("Hm, „{}“ find ich nicht. Nochmal?", missing.first().map(|p| p.display().to_string()).unwrap_or_default())], None);
    }
}

/// Splits drag-and-drop text into paths: handles `\ ` escapes and quotes.
pub fn parse_dropped(raw: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = raw.trim().chars();
    while let Some(c) = chars.next() {
        match (c, quote) {
            ('\\', None) => cur.extend(chars.next()),
            ('\'' | '"', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            (c, None) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(PathBuf::from(std::mem::take(&mut cur)));
                }
            }
            (c, _) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(PathBuf::from(cur));
    }
    out
}

pub fn converse(ui: &Ui, ws: Workspace, store: &mut UserStore, store_path: &Path, fixed: Vec<(String, Value)>) -> ExitCode {
    let ctx = Context::new(&ws, store);
    let script = match wolfgang_dialogue::load() {
        Ok(s) => s,
        Err(e) => {
            ui.error(&format!("Meine Dialog-Datei ist kaputt:\n{e}"));
            return ExitCode::from(1);
        }
    };
    let mut conv = Conversation::with_fixed(script, ctx, fastrand::u64(..), fixed);
    let mut first = true;
    let mut code = ExitCode::SUCCESS;

    loop {
        match conv.step().clone() {
            Step::Ask { lines, answers, input, can_go_back, mood, .. } => {
                ui.say(&lines, first.then_some(mood));
                first = false;
                match prompt(&answers, input.as_ref(), can_go_back) {
                    Ok(Reply::Choose(i)) => conv.choose(i),
                    Ok(Reply::Type(t)) => conv.type_answer(&t),
                    Ok(Reply::Back) => conv.back(),
                    Err(Quit) => {
                        ui.say(&["Alles klar, bis dann!".into()], None);
                        return ExitCode::from(130);
                    }
                };
            }
            Step::Start { lines, settings, rerun, .. } => {
                ui.say(&lines, None);
                let working: Vec<String> = (0..8).map(|_| conv.working_line()).collect();
                let report = match ws.plan(&settings, rerun).and_then(|plan| convert(&plan, Output::Interactive { ui, working: &working })) {
                    Ok(r) => r,
                    Err(e) => {
                        ui.error(&e.to_string());
                        return ExitCode::from(1);
                    }
                };
                if !report.failed.is_empty() {
                    code = ExitCode::from(1);
                    for (p, e) in &report.failed {
                        ui.note(&format!("✗ {}: {e}", p.display()));
                    }
                }
                conv.finished(&report);
            }
            Step::Cancelled { lines, mood } | Step::End { lines, mood } => {
                ui.say(&lines, Some(mood));
                conv.remember(store);
                if let Err(e) = store.save(store_path) {
                    ui.error(&format!("Konnte mir das nicht merken: {e}"));
                }
                return code;
            }
        }
    }
}

enum Reply {
    Choose(usize),
    Type(String),
    Back,
}

fn prompt(answers: &[wolfgang_dialogue::AnswerView], input: Option<&wolfgang_dialogue::InputView>, can_go_back: bool) -> Result<Reply, Quit> {
    loop {
        // Only a text field? Ask directly.
        if answers.is_empty() {
            if let Some(inp) = input {
                match type_in(inp) {
                    Ok(Some(t)) => return Ok(Reply::Type(t)),
                    Ok(None) if can_go_back => return Ok(Reply::Back),
                    Ok(None) => continue,
                    Err(q) => return Err(q),
                }
            }
        }
        let mut options: Vec<String> = answers.iter().map(|a| if a.current { format!("{}  ★", a.label) } else { a.label.clone() }).collect();
        if input.is_some() {
            options.push(TYPE_OWN.into());
        }
        if can_go_back {
            options.push(GO_BACK.into());
        }
        let start = answers.iter().position(|a| a.current).unwrap_or(0);
        let help = if can_go_back { "↑↓ wählen · Enter · Esc = zurück · Ctrl-C = tschüss" } else { "↑↓ wählen · Enter · Ctrl-C = tschüss" };
        let picked = Select::new("Du", options).with_starting_cursor(start).with_help_message(help).with_page_size(9).without_filtering().raw_prompt();
        match picked {
            Ok(o) if o.index < answers.len() => return Ok(Reply::Choose(o.index)),
            Ok(o) if o.value == TYPE_OWN => {
                if let Some(t) = type_in(input.expect("option only shown with input"))? {
                    return Ok(Reply::Type(t));
                }
            }
            Ok(_) => return Ok(Reply::Back),
            Err(InquireError::OperationCanceled) if can_go_back => return Ok(Reply::Back),
            Err(InquireError::OperationCanceled) => continue,
            Err(_) => return Err(Quit),
        }
    }
}

/// `Ok(None)` = Esc (back to the list).
fn type_in(inp: &wolfgang_dialogue::InputView) -> Result<Option<String>, Quit> {
    let range = match (inp.kind, inp.min, inp.max) {
        (InputKind::Number, Some(lo), Some(hi)) => format!("{lo}–{hi}{}", inp.unit.as_ref().map(|u| format!(" {u}")).unwrap_or_default()),
        (InputKind::Text, _, Some(max)) => format!("höchstens {max} Zeichen"),
        _ => String::new(),
    };
    let help = format!("{range} · Enter · Esc = zurück");
    let mut t = Text::new("Du").with_help_message(&help);
    if let Some(p) = &inp.placeholder {
        t = t.with_placeholder(p);
    }
    match t.prompt() {
        Ok(s) => Ok(Some(s)),
        Err(InquireError::OperationCanceled) => Ok(None),
        Err(_) => Err(Quit),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_dropped;
    use std::path::PathBuf;

    #[test]
    fn parses_dragged_paths() {
        assert_eq!(parse_dropped("/Users/x/Mein\\ Ordner "), vec![PathBuf::from("/Users/x/Mein Ordner")]);
        assert_eq!(parse_dropped("'/a b/c' /d/e"), vec![PathBuf::from("/a b/c"), PathBuf::from("/d/e")]);
        assert_eq!(parse_dropped("/x/Fotos\\ \\(alt\\)"), vec![PathBuf::from("/x/Fotos (alt)")]);
        assert!(parse_dropped("   ").is_empty());
    }
}
