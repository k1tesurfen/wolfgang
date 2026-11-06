//! Speech bubbles and other output helpers.

use crate::art;
use crossterm::style::Stylize;
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

    /// One bubble per line Wolfgang says; with `portrait`, his face sits left of the first one.
    pub fn say(&self, lines: &[String], portrait: Option<Mood>) {
        let lines: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            return;
        }
        if !self.color {
            for l in lines {
                println!("Wolfgang: {l}");
            }
            return;
        }
        let (face, indent) = match portrait {
            Some(mood) if self.width >= 50 => (art::portrait(mood, self.truecolor), 19),
            _ => (Vec::new(), 2),
        };
        let mut rows: Vec<String> = vec![format!("{}", "Wolfgang".bold().with(crossterm::style::Color::DarkYellow))];
        for l in lines {
            rows.extend(self.bubble(l, indent));
        }
        let height = rows.len().max(face.len());
        for i in 0..height {
            let left = match face.get(i) {
                Some(f) => format!(" {f}  "),
                None => " ".repeat(indent),
            };
            println!("{left}{}", rows.get(i).map(String::as_str).unwrap_or(""));
        }
    }

    fn bubble(&self, text: &str, indent: usize) -> Vec<String> {
        let w = self.bubble_width(indent);
        let wrapped = textwrap::wrap(text, w);
        let inner = wrapped.iter().map(|l| textwrap::core::display_width(l)).max().unwrap_or(0);
        let edge = |s: String| s.dark_grey().to_string();
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

    pub fn error(&self, text: &str) {
        if std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none() {
            eprintln!("{} {text}", "Wolfgang:".red().bold());
        } else {
            eprintln!("Wolfgang: {text}");
        }
    }
}
