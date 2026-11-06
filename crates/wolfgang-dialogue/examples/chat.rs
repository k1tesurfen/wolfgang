//! Plain-text chat with Wolfgang (dev preview until the real CLI in milestone 4).
//!
//!   cargo run --release -p wolfgang-dialogue --example chat -- PATH...
//!
//! Type the number of an answer, free text for the custom answer, or `z` to go back.

use std::io::{BufRead, Write};
use std::sync::atomic::AtomicBool;
use wolfgang_core::{execute, UserStore, Workspace};
use wolfgang_dialogue::{Context, Conversation, Step};

fn main() {
    let items: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    let store_path = UserStore::default_path();
    let mut store = UserStore::load(&store_path).expect("settings").store;
    let script = wolfgang_dialogue::load().unwrap_or_else(|e| panic!("Dialog-Datei kaputt:\n{e}"));

    if items.is_empty() {
        println!("Wolfgang: {}", Conversation::idle_line(&script, &mut fastrand::Rng::new()));
        return;
    }
    let ws = Workspace::open(&items).expect("open");
    let mut conv = Conversation::new(script, Context::new(&ws, &store), fastrand::u64(..));
    let mut stdin = std::io::stdin().lock();

    loop {
        match conv.step().clone() {
            Step::Ask { lines, answers, input, can_go_back, .. } => {
                say(&lines);
                for (i, a) in answers.iter().enumerate() {
                    println!("   {}) {}{}", i + 1, a.label, if a.current { "  ★" } else { "" });
                }
                if let Some(inp) = &input {
                    println!("   …) oder selbst eintippen{}", inp.placeholder.as_ref().map(|p| format!(" ({p})")).unwrap_or_default());
                }
                if can_go_back {
                    println!("   z) zurück");
                }
                print!("> ");
                std::io::stdout().flush().ok();
                let mut line = String::new();
                if stdin.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim();
                println!("Du: {line}");
                match line.parse::<usize>() {
                    _ if line == "z" => {
                        conv.back();
                    }
                    Ok(n) if (1..=answers.len()).contains(&n) && !(input.is_some() && line.len() > 2) => {
                        conv.choose(n - 1);
                    }
                    _ => {
                        conv.type_answer(line);
                    }
                }
            }
            Step::Start { lines, settings, rerun, .. } => {
                say(&lines);
                println!("Wolfgang: {}", conv.working_line());
                let plan = ws.plan(&settings, rerun).expect("plan");
                let report = execute(&plan, &|_| {}, &AtomicBool::new(false)).expect("run");
                conv.finished(&report);
            }
            Step::Cancelled { lines, .. } | Step::End { lines, .. } => {
                say(&lines);
                conv.remember(&mut store);
                store.save(&store_path).expect("save settings");
                return;
            }
        }
    }
}

fn say(lines: &[String]) {
    for l in lines {
        println!("Wolfgang: {l}");
    }
}
