use std::path::{Path, PathBuf};

/// Everything that can go wrong inside the engine.
///
/// Messages are German because they end up in front of users; the dialogue
/// layer wraps them in Wolfgang's voice.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Kann {} nicht als Bild lesen: {reason}", path.display())]
    Decode { path: PathBuf, reason: String },
    #[error("Beim Speichern als {format} ging was schief: {reason}")]
    Encode { format: &'static str, reason: String },
    #[error("Mit der Schrift stimmt was nicht: {0}")]
    Font(String),
    #[error("Ungültige Einstellung: {0}")]
    Settings(String),
    #[error("Abgebrochen")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

/// `map_err(io_err(path))` attaches the path to an `std::io::Error`.
pub(crate) fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io { path: path.to_path_buf(), source }
}
