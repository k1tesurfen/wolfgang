//! Which character this build is: Wolfgang, Ully or Burgunde — three apps from one
//! source (PLAN.md milestone 13). Chosen at build time with `WOLFGANG_CHARACTER`
//! (default `wolfgang`, see `build.rs`); the list is `characters.json` at the repo
//! root, which the build and release scripts read too.
//!
//! Everything else is shared: the engine, the questions, the settings file and the
//! transcription add-on (one `~/Library/Application Support/Wolfgang` for all three).

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Character {
    /// `wolfgang`, `ully`, `burgunde`
    pub key: String,
    /// What the app is called and how the character is named in every text.
    pub name: String,
    /// The app's bundle identifier (each character is its own app).
    pub identifier: String,
    /// The update feed on GitHub (`latest.json` for Wolfgang, as before 2.4).
    pub feed: String,
    /// Part of releases (every app is built and uploaded; the README links only Wolfgang).
    pub release: bool,
}

/// All characters, in the order of `characters.json`.
pub fn all() -> &'static [Character] {
    static ALL: OnceLock<Vec<Character>> = OnceLock::new();
    ALL.get_or_init(|| serde_json::from_str(include_str!("../../../characters.json")).expect("characters.json is valid"))
}

/// The character this program was built as.
pub fn current() -> &'static Character {
    all().iter().find(|c| c.key == env!("WOLFGANG_CHARACTER")).expect("checked by build.rs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn the_list_is_sound() {
        let keys: Vec<&str> = all().iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["wolfgang", "ully", "burgunde"]);
        let unique = |f: fn(&Character) -> &str| all().iter().map(f).collect::<HashSet<_>>().len() == all().len();
        assert!(unique(|c| &c.name) && unique(|c| &c.identifier) && unique(|c| &c.feed), "two apps can't share a name, id or feed");
        let w = &all()[0];
        assert_eq!((w.identifier.as_str(), w.feed.as_str()), ("de.artismedia.wolfgang", "latest.json"), "installed Wolfgangs keep finding their updates");
        assert!(w.release);
    }

    #[test]
    fn this_build_is_one_of_them() {
        assert!(all().contains(current()));
    }
}
