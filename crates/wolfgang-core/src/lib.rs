//! Wolfgang's conversion engine, shared by the CLI and the desktop app:
//! images, fonts, and (via the bundled ffmpeg) videos and audio.
//!
//! Typical flow:
//! 1. [`Workspace::open`] with whatever was dropped on the desk
//! 2. [`Workspace::summary`] → what Wolfgang tells the user about the pile
//! 3. the conversation produces [`RunSettings`]
//! 4. [`Workspace::plan`] → [`run::execute`] → [`run::Report`]

pub mod error;
pub mod font;
pub mod image;
pub mod kind;
pub mod media;
pub mod naming;
pub mod run;
pub mod runlog;
pub mod scan;
pub mod settings;
pub mod store;
pub mod workspace;

pub use error::{Error, Result};
pub use kind::FileKind;
pub use run::{execute, Event, Report};
pub use settings::{
    AudioContent, AudioFormat, AudioSettings, FontSettings, ImageFormat, ImageSettings, Naming, RunSettings, VideoFormat, VideoQuality, VideoSettings,
    WebFormat, WebSettings,
};
pub use store::UserStore;
pub use workspace::{Plan, RerunPolicy, Summary, Workspace};
