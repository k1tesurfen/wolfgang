//! Speech bubbles and other output helpers.

use crate::art;
use crossterm::style::{Color, Stylize};
use std::io::IsTerminal;
use wolfgang_dialogue::Mood;

pub struct Ui {
    pub color: bool,
    truecolor: bool,
    width: usize,
}

impl Ui {
    pub fn new() -> Self {
        let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        let width = crossterm::terminal::size().map(|(w, _)| w as usize).ok().filter(|w| *w >= 40).unwrap_or(80);
        Self { color, truecolor: art::truecolor(), width }
    }

    fn bubble_width(&self, indent: usize) -> usize {
        self.width.saturating_sub(indent + 4).clamp(24, 72)
    }

    /// Everything Wolfgang says in one step goes into one bubble (one paragraph
    /// per line); with `portrait`, his face sits left of it.
    pub fn say(&self, lines: &[String], portrait: Option<Mood>) {
        for row in self.render(lines, portrait, Color::DarkGrey) {
            println!("{row}");
        }
    }

    /// Like [`Ui::say`], but on stderr and with a red frame.
    pub fn error(&self, text: &str) {
        let colored = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        if !colored {
            eprintln!("{}: {text}", name());
            return;
        }
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        for row in self.render_colored(&lines, None, Color::DarkRed) {
            eprintln!("{row}");
        }
    }

    fn render(&self, lines: &[String], portrait: Option<Mood>, frame: Color) -> Vec<String> {
        if !self.color {
            return lines.iter().filter(|l| !l.trim().is_empty()).map(|l| format!("{}: {l}", name())).collect();
        }
        self.render_colored(lines, portrait, frame)
    }

    fn render_colored(&self, lines: &[String], portrait: Option<Mood>, frame: Color) -> Vec<String> {
        let lines: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() {
            return Vec::new();
        }
        let (face, indent) = match portrait {
            Some(mood) if self.width >= 50 => (art::portrait(mood, self.truecolor), 19),
            _ => (Vec::new(), 2),
        };
        let mut rows = vec![format!("{}", name().bold().with(Color::DarkYellow))];
        rows.extend(self.bubble(&lines, indent, frame));
        let height = rows.len().max(face.len());
        (0..height)
            .map(|i| {
                let left = match face.get(i) {
                    Some(f) => format!(" {f}  "),
                    None => " ".repeat(indent),
                };
                format!("{left}{}", rows.get(i).map(String::as_str).unwrap_or(""))
            })
            .collect()
    }

    fn bubble(&self, paragraphs: &[&str], indent: usize, frame: Color) -> Vec<String> {
        let w = self.bubble_width(indent);
        let wrapped: Vec<String> = paragraphs.iter().flat_map(|p| textwrap::wrap(p, w)).map(|l| l.into_owned()).collect();
        let inner = wrapped.iter().map(|l| textwrap::core::display_width(l)).max().unwrap_or(0);
        let edge = |s: String| s.with(frame).to_string();
        let mut out = vec![edge(format!("╭{}╮", "─".repeat(inner + 2)))];
        for l in &wrapped {
            let pad = inner - textwrap::core::display_width(l);
            out.push(format!("{} {}{} {}", edge("│".into()), l, " ".repeat(pad), edge("│".into())));
        }
        out.push(edge(format!("╰{}╯", "─".repeat(inner + 2))));
        out
    }

    /// Quiet info line (not Wolfgang speaking).
    pub fn note(&self, text: &str) {
        if self.color {
            println!("  {}", text.dark_grey());
        } else {
            println!("{text}");
        }
    }

}

/// Wolfgang, Ully or Burgunde: the character of the app this `wolfgang` came with.
fn name() -> &'static str {
    &wolfgang_core::character::current().name
}
