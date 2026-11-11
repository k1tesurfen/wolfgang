//! The dialogue file format (`dialogue/de.yaml`) and its validation.
//!
//! ```yaml
//! start: greet              # entry node
//! after_run: done           # node after a finished run
//! after_cancel: stopped     # node after a cancelled run
//! idle: ["Leg mir was auf den Tisch."]   # lines for an empty desk
//! working: ["Moment …"]                  # lines while converting
//! check:                                  # „Guck mal, die Seite“ (a pasted URL)
//!   looking: ["Moment, ich guck mir {host} an …"]
//!   fine: […]   odd: […]   dns: […]   server: […]   wordpress: […]   # per verdict, with {finding}
//! nodes:
//!   greet:
//!     mood: happy
//!     say:                                # each entry is one speech bubble
//!       - "Always said"
//!       - ["Variant A", "Variant B"]      # one picked at random
//!       - when: [has_fonts, "!has_images"]
//!         text: "Only if the condition holds, {fonts:eine Schrift:Schriften}"
//!     set: { mode: web }                  # applied when entering
//!     go:                                 # no answers: continue automatically
//!       - { when: [nothing_to_do], to: nothing }
//!       - { to: mode }
//!     answers:
//!       - { label: "Fürs Web", set: { mode: web }, to: web_format }
//!       - { label: "{preset}", each: presets, set: { preset: "{preset}" }, to: confirm }
//!     input: { kind: number, min: 16, max: 16384, unit: px, set: web.max_side, to: next, invalid: ["…"] }
//!     foreach: folders_without_keywords   # node repeats per item ({folder}), then goes to `then`
//!     then: separator
//! ```
//! Targets starting with `@` end the conversation: `@start`, `@cancel`, `@end`.

use crate::context::{FLAGS, NUMBER_VARS, TEXT_VARS};
use crate::draft::KEYS;
use crate::template;
use serde::Deserialize;
use std::collections::BTreeMap;

pub type Value = serde_norway::Value;

#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    #[error("YAML: {0}")]
    Yaml(#[from] serde_norway::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub start: String,
    pub after_run: String,
    pub after_cancel: String,
    #[serde(default)]
    pub idle: Vec<String>,
    #[serde(default)]
    pub working: Vec<String>,
    #[serde(default)]
    pub check: CheckLines,
    pub nodes: BTreeMap<String, Node>,
}

/// What Wolfgang says about a checked URL. `{host}`, `{finding}` (the most
/// important finding) and `{name}` can be used; one variant is picked at random.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckLines {
    pub looking: Vec<String>,
    pub fine: Vec<String>,
    pub odd: Vec<String>,
    pub dns: Vec<String>,
    pub server: Vec<String>,
    pub wordpress: Vec<String>,
}

impl CheckLines {
    const VARS: &[&str] = &["host", "finding", "name"];

    fn all(&self) -> [(&'static str, &Vec<String>); 6] {
        [("looking", &self.looking), ("fine", &self.fine), ("odd", &self.odd), ("dns", &self.dns), ("server", &self.server), ("wordpress", &self.wordpress)]
    }

    /// A line for `key` (`looking` or a verdict), filled in.
    pub fn say(&self, key: &str, host: &str, finding: &str) -> String {
        let lines = self.all().into_iter().find(|(k, _)| *k == key).map(|(_, l)| l.as_slice()).unwrap_or_default();
        let Some(line) = (!lines.is_empty()).then(|| &lines[fastrand::usize(..lines.len())]) else { return String::new() };
        let name = wolfgang_core::character::current().name.clone();
        template::render(line, |v| match v {
            "host" => Some(template::Var::Text(host.to_owned())),
            "finding" => Some(template::Var::Text(finding.to_owned())),
            "name" => Some(template::Var::Text(name.clone())),
            _ => None,
        })
        .trim()
        .to_owned()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    #[serde(default)]
    pub mood: Mood,
    #[serde(default)]
    pub say: Vec<Line>,
    #[serde(default)]
    pub set: BTreeMap<String, Value>,
    #[serde(default)]
    pub go: Vec<Go>,
    #[serde(default)]
    pub answers: Vec<Answer>,
    pub input: Option<Input>,
    pub foreach: Option<Foreach>,
    pub then: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    #[default]
    Neutral,
    Happy,
    Thinking,
    Surprised,
    Proud,
    Sorry,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Line {
    One(String),
    Variants(Vec<String>),
    When { when: Vec<String>, text: Texts },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Texts {
    One(String),
    Variants(Vec<String>),
}

impl Line {
    pub fn condition(&self) -> &[String] {
        match self {
            Line::When { when, .. } => when,
            _ => &[],
        }
    }

    pub fn variants(&self) -> Vec<&str> {
        match self {
            Line::One(s) | Line::When { text: Texts::One(s), .. } => vec![s],
            Line::Variants(v) | Line::When { text: Texts::Variants(v), .. } => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Go {
    #[serde(default)]
    pub when: Vec<String>,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub label: String,
    #[serde(default)]
    pub when: Vec<String>,
    #[serde(default)]
    pub set: BTreeMap<String, Value>,
    /// Where to go; may be omitted inside `foreach` nodes (then `then` is used after the last item).
    pub to: Option<String>,
    /// Expand into one answer per item; the item is available as `{preset}`.
    pub each: Option<Each>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Each {
    Presets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Foreach {
    FoldersWithoutKeywords,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Number,
    Text,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub kind: InputKind,
    pub set: String,
    pub to: Option<String>,
    #[serde(default)]
    pub min: Option<u32>,
    #[serde(default)]
    pub max: Option<u32>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    /// Text inputs: maximum length in characters.
    #[serde(default)]
    pub max_len: Option<usize>,
    /// What Wolfgang says to invalid input. `{input}`, `{min}`, `{max}` are available.
    #[serde(default)]
    pub invalid: Vec<Line>,
}

/// Placeholders only valid in specific spots.
const ITEM_VARS: &[&str] = &["preset", "folder"];
const INPUT_VARS: &[&str] = &["input", "min", "max"];

impl Script {
    pub fn parse(yaml: &str) -> Result<Self, ScriptError> {
        let script: Script = serde_norway::from_str(yaml)?;
        script.validate()?;
        Ok(script)
    }

    /// Catches typos in the dialogue file at load time instead of mid-conversation.
    fn validate(&self) -> Result<(), ScriptError> {
        let mut errors = Vec::new();
        let target_ok = |t: &str| t.starts_with('@') && ["@start", "@cancel", "@end"].contains(&t) || self.nodes.contains_key(t);
        let check_target = |errors: &mut Vec<String>, at: &str, t: &str| {
            if !target_ok(t) {
                errors.push(format!("{at}: Ziel '{t}' gibt es nicht"));
            }
        };
        let check_flags = |errors: &mut Vec<String>, at: &str, when: &[String]| {
            for f in when {
                if !FLAGS.contains(&f.trim_start_matches('!')) {
                    errors.push(format!("{at}: Bedingung '{f}' kenne ich nicht"));
                }
            }
        };
        let check_text = |errors: &mut Vec<String>, at: &str, text: &str, extra: &[&str]| {
            if let Err(e) = template::check(text, |name| {
                if NUMBER_VARS.contains(&name) {
                    Some(true)
                } else if TEXT_VARS.contains(&name) || ITEM_VARS.contains(&name) || extra.contains(&name) {
                    Some(false)
                } else {
                    None
                }
            }) {
                errors.push(format!("{at}: {e}"));
            }
        };
        let check_set = |errors: &mut Vec<String>, at: &str, set: &BTreeMap<String, Value>| {
            for k in set.keys() {
                if !KEYS.contains(&k.as_str()) {
                    errors.push(format!("{at}: Einstellung '{k}' kenne ich nicht"));
                }
            }
        };

        for t in [&self.start, &self.after_run, &self.after_cancel] {
            check_target(&mut errors, "Kopf", t);
        }
        for (i, l) in self.idle.iter().chain(&self.working).enumerate() {
            check_text(&mut errors, &format!("idle/working #{i}"), l, &[]);
        }
        // Without a `check:` section the URL check has nothing to say; with one, every verdict needs a line.
        let has_check = self.check.all().iter().any(|(_, l)| !l.is_empty());
        for (key, lines) in self.check.all() {
            if has_check && lines.is_empty() {
                errors.push(format!("check.{key}: keine Zeile"));
            }
            for l in lines {
                if let Err(e) = template::check(l, |n| CheckLines::VARS.contains(&n).then_some(false)) {
                    errors.push(format!("check.{key}: {e}"));
                }
            }
        }
        for (id, n) in &self.nodes {
            let at = |what: &str| format!("Knoten '{id}' {what}");
            for l in &n.say {
                check_flags(&mut errors, &at("say"), l.condition());
                for v in l.variants() {
                    check_text(&mut errors, &at("say"), v, &[]);
                }
            }
            check_set(&mut errors, &at("set"), &n.set);
            for g in &n.go {
                check_flags(&mut errors, &at("go"), &g.when);
                check_target(&mut errors, &at("go"), &g.to);
            }
            let has_choices = !n.answers.is_empty() || n.input.is_some();
            if has_choices != n.go.is_empty() {
                errors.push(at("braucht entweder 'go' oder Antworten/Eingabe"));
            }
            if n.foreach.is_some() != n.then.is_some() {
                errors.push(at("'foreach' und 'then' gehören zusammen"));
            }
            if let Some(t) = &n.then {
                check_target(&mut errors, &at("then"), t);
            }
            for a in &n.answers {
                check_flags(&mut errors, &at("answer"), &a.when);
                check_text(&mut errors, &at("answer"), &a.label, &[]);
                check_set(&mut errors, &at("answer"), &a.set);
                match &a.to {
                    Some(t) => check_target(&mut errors, &at("answer"), t),
                    None if n.foreach.is_none() => errors.push(at(&format!("Antwort '{}' braucht 'to'", a.label))),
                    None => {}
                }
            }
            if let Some(inp) = &n.input {
                if !KEYS.contains(&inp.set.as_str()) {
                    errors.push(at(&format!("input: Einstellung '{}' kenne ich nicht", inp.set)));
                }
                match &inp.to {
                    Some(t) => check_target(&mut errors, &at("input"), t),
                    None if n.foreach.is_none() => errors.push(at("input braucht 'to'")),
                    None => {}
                }
                if let (Some(lo), Some(hi)) = (inp.min, inp.max) {
                    if lo > hi {
                        errors.push(at("input: min > max"));
                    }
                }
                for l in &inp.invalid {
                    for v in l.variants() {
                        check_text(&mut errors, &at("input.invalid"), v, INPUT_VARS);
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ScriptError::Invalid(errors.join("\n")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = r#"
start: a
after_run: a
after_cancel: a
nodes:
  a:
    say: ["Hallo {images:ein Foto:Fotos}"]
    answers:
      - { label: Los, set: { mode: web }, to: "@start" }
"#;

    #[test]
    fn parses_a_minimal_script() {
        let s = Script::parse(MINI).unwrap();
        assert_eq!(s.nodes.len(), 1);
    }

    #[test]
    fn reports_all_mistakes_at_once() {
        let bad = MINI
            .replace("to: \"@start\"", "to: nirgendwo")
            .replace("mode: web", "modus: web")
            .replace("{images:", "{bilder:");
        let err = Script::parse(&bad).unwrap_err().to_string();
        assert!(err.contains("'nirgendwo'"), "{err}");
        assert!(err.contains("'modus'"), "{err}");
        assert!(err.contains("bilder"), "{err}");
    }

    #[test]
    fn rejects_unknown_fields_and_flags() {
        assert!(Script::parse(&MINI.replace("say:", "sag:")).is_err());
        let with_flag = MINI.replace("{ label: Los,", "{ label: Los, when: [hat_bilder],");
        assert!(Script::parse(&with_flag).unwrap_err().to_string().contains("hat_bilder"));
    }
}
