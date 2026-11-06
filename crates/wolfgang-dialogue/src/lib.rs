//! Wolfgang's conversation, shared by CLI and app.
//!
//! The dialogue itself lives in `dialogue/de.yaml` (embedded at build time).
//! This crate evaluates it: [`Conversation`] hands out [`Step`]s to render and
//! takes the user's choices.

pub mod context;
pub mod conversation;
pub mod draft;
pub mod script;
pub mod template;

pub use context::Context;
pub use conversation::{AnswerView, Conversation, InputView, Step};
pub use script::{Mood, Script, ScriptError};

use std::sync::Arc;

/// Lets dialogue writers try changes without rebuilding: point it at a YAML file.
pub const DIALOGUE_ENV: &str = "WOLFGANG_DIALOGUE";

const BUILTIN: &str = include_str!("../../../dialogue/de.yaml");

/// Loads the dialogue: `$WOLFGANG_DIALOGUE` if set, else the built-in German one.
pub fn load() -> Result<Arc<Script>, ScriptError> {
    let yaml = match std::env::var_os(DIALOGUE_ENV) {
        Some(path) => std::fs::read_to_string(&path).map_err(|e| ScriptError::Invalid(format!("{}: {e}", path.to_string_lossy())))?,
        None => BUILTIN.to_owned(),
    };
    Script::parse(&yaml).map(Arc::new)
}

/// The built-in dialogue only.
pub fn builtin() -> Arc<Script> {
    Arc::new(Script::parse(BUILTIN).expect("built-in dialogue is validated by tests"))
}
