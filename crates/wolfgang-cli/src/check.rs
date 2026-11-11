//! `wolfgang check <url>`: „Guck mal, die Seite“ in the terminal.

use crate::ui::Ui;
use std::io::IsTerminal;
use std::process::ExitCode;
use wolfgang_check::{Stage, Verdict};
use wolfgang_dialogue::Mood;

/// Exit codes: 0 nothing wrong (or only something odd), 1 a problem, 2 not a URL.
pub fn run(ui: &Ui, input: &str, json: bool) -> ExitCode {
    let script = match wolfgang_dialogue::load() {
        Ok(s) => s,
        Err(e) => {
            ui.error(&format!("Mein Text ist kaputt: {e}"));
            return ExitCode::from(1);
        }
    };
    let host = wolfgang_check::parse(input).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
    let spinner = (!json && std::io::stderr().is_terminal()).then(|| {
        ui.say(&[script.check.say("looking", &host, "")], Some(Mood::Thinking));
        let pb = indicatif::ProgressBar::new_spinner().with_style(
            indicatif::ProgressStyle::with_template("  {spinner:.yellow} {msg}").expect("valid template").tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        pb
    });
    let stage = |s: Stage| {
        if let Some(pb) = &spinner {
            pb.set_message(match s {
                Stage::Dns => "frag die Nameserver …",
                Stage::Path => "folge der Anfrage …",
                Stage::Addresses => "klopf bei jedem Server einzeln an …",
            });
        }
    };
    let result = wolfgang_check::check(input, &stage);
    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }
    let c = match result {
        Ok(c) => c,
        Err(e) => {
            ui.error(&e);
            return ExitCode::from(2);
        }
    };
    let finding = c.headline().map(|f| f.text.clone()).unwrap_or_default();
    let says = script.check.say(c.verdict.key(), &c.host, &finding);
    let name = wolfgang_core::character::current().name.clone();
    let report = wolfgang_check::report::text(&c, &name);
    if json {
        let mut v = serde_json::to_value(&c).expect("serializable");
        v["says"] = says.into();
        v["report"] = report.into();
        println!("{}", serde_json::to_string_pretty(&v).expect("serializable"));
    } else {
        let mood = match c.verdict {
            Verdict::Fine => Mood::Happy,
            Verdict::Odd => Mood::Thinking,
            _ => Mood::Sorry,
        };
        ui.say(&[says], Some(mood));
        println!("\n{report}");
    }
    if matches!(c.verdict, Verdict::Fine | Verdict::Odd) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
