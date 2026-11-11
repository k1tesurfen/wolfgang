//! Command line arguments and how they turn into dialogue answers.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use wolfgang_core::workspace::Piles;
use wolfgang_core::{RerunPolicy, RunSettings, UserStore};
use wolfgang_dialogue::draft::{Draft, Mode};
use wolfgang_dialogue::script::Value;

#[derive(Parser, Debug)]
#[command(
    name = "wolfgang",
    version,
    // No pronouns for the character: the same text is used for Ully and Burgunde
    // (main.rs swaps the name).
    about = "Wolfgang wandelt um: Bilder fürs Web oder hochwertig, Videos und Audio fürs Web, Bilder aus PDFs, Schriften zu WOFF2. Und schreibt Besprechungen auf.",
    long_about = "Wolfgang wandelt um: Bilder fürs Web oder hochwertig, Videos und Audio fürs Web, Bilder aus PDFs, Schriften zu WOFF2. \
        Und schreibt auf, was in Besprechungen gesagt wird (--video-format/--audio-format transcript).\n\n\
        Im Terminal gibt's ein kurzes Gespräch: Wolfgang fragt, was du willst. Optionen beantworten die Fragen vorab; \
        mit --yes (oder ohne Terminal) wird nichts gefragt, dann gelten deine Standards.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(Args, Debug, Default)]
pub struct RunArgs {
    /// Ordner oder Dateien (ohne: Wolfgang fragt danach)
    pub paths: Vec<PathBuf>,

    /// Fürs Web vorbereiten (klein, schnell, SEO-Namen)
    #[arg(long, conflicts_with = "hq")]
    pub web: bool,
    /// Hochwertig behalten (Originalgröße, Originalnamen)
    #[arg(long)]
    pub hq: bool,
    /// Bildformat (fürs Web nur webp oder avif)
    #[arg(short, long, value_enum)]
    pub format: Option<FormatArg>,
    /// Längste Seite in Pixeln (Web)
    #[arg(short = 'd', long, visible_alias = "dimension", value_parser = clap::value_parser!(u32).range(100..=8000))]
    pub size: Option<u32>,
    /// Qualität 1–100 (Web)
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub quality: Option<u8>,
    /// Vorsilbe für Dateinamen (Web)
    #[arg(short, long)]
    pub name: Option<String>,
    /// Stichworte für Ordner ohne Stichwortzettel (Web)
    #[arg(short, long)]
    pub keywords: Option<String>,
    /// Worttrennung in Dateinamen (Web)
    #[arg(short, long, value_enum)]
    pub separator: Option<SeparatorArg>,
    /// Alten Dateinamen anhängen (Web)
    #[arg(short, long)]
    pub append: bool,
    /// Videoformat
    #[arg(long, value_enum)]
    pub video_format: Option<VideoFormatArg>,
    /// Videogröße: 720, 1080, original oder die kürzere Seite in Pixeln
    #[arg(long, value_parser = parse_video_size)]
    pub video_size: Option<VideoSizeArg>,
    /// Videoqualität
    #[arg(long, value_enum)]
    pub video_quality: Option<VideoQualityArg>,
    /// Videos ohne Ton
    #[arg(long)]
    pub no_sound: bool,
    /// Kein Vorschaubild zu Videos
    #[arg(long)]
    pub no_poster: bool,
    /// Was hinter durchsichtige Stellen animierter GIFs kommt
    #[arg(long, value_enum)]
    pub gif_background: Option<BackgroundArg>,
    /// Audioformat
    #[arg(long, value_enum)]
    pub audio_format: Option<AudioFormatArg>,
    /// Was ist auf den Audiodateien?
    #[arg(long = "audio", value_enum)]
    pub audio_content: Option<AudioContentArg>,
    /// Beim Aufschreiben Zeitangaben vor jeden Absatz
    #[arg(long, conflicts_with = "no_timestamps")]
    pub timestamps: bool,
    /// Beim Aufschreiben keine Zeitangaben
    #[arg(long)]
    pub no_timestamps: bool,
    /// Gespeicherte Vorlage benutzen
    #[arg(short, long)]
    pub preset: Option<String>,
    /// Nur einen Stapel bearbeiten
    #[arg(long, value_enum)]
    pub only: Option<OnlyArg>,
    /// Auch schon umgewandelte Dateien nochmal machen
    #[arg(long)]
    pub all: bool,
    /// Nicht fragen: Standards + Optionen nehmen
    #[arg(short, long)]
    pub yes: bool,
    /// Maschinenlesbare Ausgabe (eine JSON-Zeile pro Ereignis), fragt nie
    #[arg(long)]
    pub json: bool,
    /// Wie früher: alte Umbenennungen rückgängig machen (= legacy-reset)
    #[arg(short, long, hide = true)]
    pub reset: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Gespeicherte Vorlagen anzeigen oder löschen
    Presets {
        #[command(subcommand)]
        action: Option<PresetAction>,
    },
    /// Deine Standards anzeigen oder ändern
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },
    /// Zusatzmodul fürs Aufschreiben (≈ 575 MB): anzeigen, installieren, entfernen
    Transcription {
        #[command(subcommand)]
        action: Option<TranscriptionAction>,
    },
    /// Umbenennungen des alten Wolfgang (1.x) an Originaldateien rückgängig machen
    LegacyReset {
        /// Ordner, in dem der alte Wolfgang lief
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Ohne Nachfrage umbenennen
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum PresetAction {
    /// Alle Vorlagen anzeigen
    List,
    /// Eine Vorlage löschen
    Remove { name: String },
}

#[derive(Subcommand, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionAction {
    /// Ist es installiert?
    Status,
    /// Herunterladen und installieren
    Install,
    /// Wieder entfernen
    Remove,
}

#[derive(Subcommand, Debug)]
pub enum SettingsAction {
    /// Standards anzeigen
    Show,
    /// Einen Standard ändern, z. B. `wolfgang settings set web-format avif`
    Set { key: SettingKey, value: String },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKey {
    WebFormat,
    Size,
    Quality,
    HqFormat,
    Separator,
    VideoFormat,
    VideoSize,
    VideoQuality,
    AudioFormat,
    Sound,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatArg {
    Jpg,
    Webp,
    Avif,
    Png,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeparatorArg {
    Dash,
    Underscore,
    Caps,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnlyArg {
    Images,
    Videos,
    Audio,
    Pdfs,
    Fonts,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoFormatArg {
    Mp4,
    Webm,
    /// Nur aufschreiben, was gesagt wird (braucht das Zusatzmodul)
    Transcript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoSizeArg {
    Original,
    ShorterSide(u32),
}

fn parse_video_size(s: &str) -> Result<VideoSizeArg, String> {
    if s.eq_ignore_ascii_case("original") {
        return Ok(VideoSizeArg::Original);
    }
    let range = wolfgang_core::settings::VIDEO_SIZE_RANGE;
    match s.trim_end_matches(['p', 'P']).parse::<u32>() {
        Ok(n) if range.contains(&n) => Ok(VideoSizeArg::ShorterSide(n)),
        _ => Err(format!("720, 1080, original oder eine Zahl von {} bis {}", range.start(), range.end())),
    }
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoQualityArg {
    Smaller,
    Balanced,
    Nicer,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundArg {
    White,
    Black,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormatArg {
    Mp3,
    Aac,
    /// Nur aufschreiben, was gesagt wird (braucht das Zusatzmodul)
    Transcript,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioContentArg {
    Speech,
    Music,
    Best,
}

/// A value-enum's name as written on the command line (`smaller`, `mp3`, …).
fn arg_name(v: impl ValueEnum) -> String {
    v.to_possible_value().expect("no skipped variants").get_name().to_owned()
}

fn yaml(s: &str) -> Value {
    serde_norway::from_str(s).expect("static yaml")
}

fn text(s: &str) -> Value {
    Value::String(s.to_owned())
}

fn num(n: u32) -> Value {
    Value::Number(n.into())
}

impl FormatArg {
    fn key(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Png => "png",
        }
    }
}

impl RunArgs {
    fn web_only_flags(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.size.is_some() {
            v.push("--size");
        }
        if self.quality.is_some() {
            v.push("--quality");
        }
        if self.name.is_some() {
            v.push("--name");
        }
        if self.keywords.is_some() {
            v.push("--keywords");
        }
        if self.separator.is_some() {
            v.push("--separator");
        }
        if self.append {
            v.push("--append");
        }
        v
    }

    /// Flags as pre-given dialogue answers. Errors are usage errors (German).
    pub fn fixed(&self, store: &UserStore) -> Result<Vec<(String, Value)>, String> {
        let web_flags = self.web_only_flags();
        if self.hq && !web_flags.is_empty() {
            return Err(format!("{} gilt nur fürs Web, nicht mit --hq", web_flags.join(", ")));
        }
        let format_is_web = matches!(self.format, Some(FormatArg::Webp | FormatArg::Avif));
        let mode = if self.hq || (self.format.is_some() && !format_is_web) {
            Some("hq")
        } else if self.web || !web_flags.is_empty() {
            Some("web")
        } else {
            None
        };
        if mode == Some("hq") && !web_flags.is_empty() {
            return Err(format!("{} gilt nur fürs Web; JPG und PNG gibt's nur bei „hochwertig“", web_flags.join(", ")));
        }

        let mut f: Vec<(String, Value)> = Vec::new();
        let mut put = |k: &str, v: Value| f.push((k.to_owned(), v));
        if let Some(name) = &self.preset {
            let p = store.preset(name).ok_or_else(|| {
                let known: Vec<_> = store.presets.iter().map(|p| p.name.as_str()).collect();
                if known.is_empty() {
                    format!("Vorlage „{name}“ gibt's nicht (du hast noch keine)")
                } else {
                    format!("Vorlage „{name}“ gibt's nicht. Vorhanden: {}", known.join(", "))
                }
            })?;
            put("preset", text(&p.name));
        } else if mode.is_some() || self.format.is_some() {
            put("use_last", yaml("false"));
        }
        if let Some(m) = mode {
            put("mode", text(m));
        }
        if let Some(fmt) = self.format {
            if mode != Some("web") {
                put("hq.format", text(fmt.key()));
            }
            if format_is_web && mode != Some("hq") {
                put("web.format", text(fmt.key()));
            }
        }
        if let Some(s) = self.size {
            put("web.max_side", num(s));
        }
        if let Some(q) = self.quality {
            put("web.quality", num(q as u32));
        }
        if let Some(n) = &self.name {
            put("naming.prefix", text(n));
        }
        if let Some(k) = &self.keywords {
            put("naming.keywords_all", text(k));
        }
        if let Some(s) = self.separator {
            put("naming.separator", text(match s {
                SeparatorArg::Dash => "dash",
                SeparatorArg::Underscore => "underscore",
                SeparatorArg::Caps => "caps",
            }));
        }
        if self.append {
            put("naming.append_original", yaml("true"));
        }
        if let Some(v) = self.video_format {
            put("video.format", text(&arg_name(v)));
        }
        match self.video_size {
            Some(VideoSizeArg::Original) => put("video.size", text("original")),
            Some(VideoSizeArg::ShorterSide(n)) => put("video.size", num(n)),
            None => {}
        }
        if let Some(q) = self.video_quality {
            put("video.quality", text(&arg_name(q)));
        }
        if self.no_sound {
            put("video.sound", yaml("false"));
        }
        if self.no_poster {
            put("video.poster", yaml("false"));
        }
        if let Some(b) = self.gif_background {
            put("video.background", text(&arg_name(b)));
        }
        if let Some(a) = self.audio_format {
            put("audio.format", text(&arg_name(a)));
        }
        if let Some(c) = self.audio_content {
            put("audio.content", text(&arg_name(c)));
        }
        if self.timestamps || self.no_timestamps {
            put("transcript.timestamps", yaml(if self.timestamps { "true" } else { "false" }));
        }
        if self.all {
            put("rerun", text("all"));
        }
        Ok(f)
    }

    /// „Nur aufschreiben“ was asked for (it needs the add-on).
    pub fn wants_transcripts(&self) -> bool {
        self.video_format == Some(VideoFormatArg::Transcript) || self.audio_format == Some(AudioFormatArg::Transcript)
    }

    /// Settings without a conversation: defaults, then preset, then flags.
    /// Without any mode choice, images are prepared for the web; videos and
    /// audio get Wolfgang's defaults.
    pub fn settings(&self, store: &UserStore, piles: Piles) -> Result<(RunSettings, RerunPolicy), String> {
        let mut draft = Draft::new(&store.defaults);
        let fixed = self.fixed(store)?;
        // Actions first (preset), then plain values so flags win.
        for (k, v) in fixed.iter().filter(|(k, _)| k == "preset").chain(fixed.iter().filter(|(k, _)| k != "preset")) {
            draft.apply(k, v, store, None)?;
        }
        if draft.mode.is_none() {
            draft.mode = Some(Mode::Web);
        }
        draft.videos_asked = true;
        draft.audio_asked = true;
        let settings = draft.settings(piles);
        settings.validate().map_err(|e| e.to_string())?;
        Ok((settings, draft.rerun))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wolfgang_core::{ImageFormat, ImageSettings, WebFormat};

    const IMAGES: Piles = Piles { images: true, videos: false, audio: false, pdfs: false, fonts: false };

    fn args(v: &[&str]) -> RunArgs {
        Cli::try_parse_from(std::iter::once("wolfgang").chain(v.iter().copied())).unwrap().run
    }

    #[test]
    fn web_flags_imply_web_mode() {
        let store = UserStore::default();
        let (s, _) = args(&["--size", "2000", "-n", "kunde", "x"]).settings(&store, IMAGES).unwrap();
        let Some(ImageSettings::Web(w)) = s.images else { panic!() };
        assert_eq!((w.max_side, w.naming.prefix.as_str(), w.format), (2000, "kunde", WebFormat::Webp));
    }

    #[test]
    fn jpg_implies_high_quality_and_conflicts_with_web_flags() {
        let store = UserStore::default();
        let (s, _) = args(&["-f", "png", "x"]).settings(&store, IMAGES).unwrap();
        assert_eq!(s.images, Some(ImageSettings::HighQuality { format: ImageFormat::Png }));
        assert!(args(&["-f", "jpg", "--size", "800"]).fixed(&store).is_err());
        assert!(args(&["--hq", "-q", "50"]).fixed(&store).is_err());
        assert!(Cli::try_parse_from(["wolfgang", "--web", "--hq"]).is_err());
    }

    #[test]
    fn avif_without_mode_answers_both_format_questions() {
        let f = args(&["-f", "avif"]).fixed(&UserStore::default()).unwrap();
        let keys: Vec<_> = f.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["use_last", "hq.format", "web.format"]);
    }

    #[test]
    fn preset_then_flags() {
        let mut store = UserStore::default();
        store.save_preset("Druck", RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Png }), ..Default::default() }).unwrap();
        let (s, _) = args(&["-p", "druck", "x"]).settings(&store, Piles { images: true, fonts: true, ..Default::default() }).unwrap();
        assert_eq!(s.images, Some(ImageSettings::HighQuality { format: ImageFormat::Png }));
        assert!(s.fonts.is_some());
        let err = args(&["-p", "nope"]).fixed(&store).unwrap_err();
        assert!(err.contains("Vorhanden: Druck"), "{err}");
    }

    #[test]
    fn video_and_audio_flags() {
        let store = UserStore::default();
        let all = Piles::ALL;
        let (s, _) = args(&["x"]).settings(&store, all).unwrap();
        assert_eq!(s.videos, Some(wolfgang_core::VideoSettings::default()), "Wolfgang's defaults without a conversation");
        assert_eq!(s.audio, Some(wolfgang_core::AudioSettings::default()));

        let a = args(&["--video-format", "webm", "--video-size", "720p", "--video-quality", "nicer", "--no-sound", "--no-poster", "--audio-format", "aac", "--audio", "speech", "x"]);
        let (s, _) = a.settings(&store, all).unwrap();
        let v = s.videos.unwrap();
        assert_eq!((v.format, v.size, v.quality, v.sound, v.poster), (wolfgang_core::VideoFormat::Webm, Some(720), wolfgang_core::VideoQuality::Nicer, false, false));
        assert_eq!(s.audio.unwrap(), wolfgang_core::AudioSettings { format: wolfgang_core::AudioFormat::Aac, content: wolfgang_core::AudioContent::Speech });
        let (s, _) = args(&["--video-size", "original", "x"]).settings(&store, all).unwrap();
        assert_eq!(s.videos.unwrap().size, None);
        assert!(Cli::try_parse_from(["wolfgang", "--video-size", "5"]).is_err());
        assert!(args(&["--video-format", "webm", "x"]).settings(&store, IMAGES).unwrap().0.videos.is_none(), "no videos on the desk");

        let a = args(&["--audio-format", "transcript", "--no-timestamps", "x"]);
        assert!(a.wants_transcripts());
        let (s, _) = a.settings(&store, all).unwrap();
        assert_eq!((s.audio, s.audio_transcript), (None, Some(wolfgang_core::TranscriptSettings { timestamps: false })));
        assert!(s.videos.is_some() && s.video_transcript.is_none(), "videos are still converted");
        assert!(Cli::try_parse_from(["wolfgang", "--timestamps", "--no-timestamps"]).is_err());
    }

    #[test]
    fn old_short_flags_still_work() {
        let a = args(&["-n", "holiday", "-d", "1600", "-q", "85", "-s", "caps", "-a", "trip"]);
        assert_eq!((a.size, a.quality, a.separator, a.append), (Some(1600), Some(85), Some(SeparatorArg::Caps), true));
        assert!(args(&["-r", "trip"]).reset);
    }

    #[test]
    fn subcommands_parse() {
        assert!(matches!(Cli::try_parse_from(["wolfgang", "presets"]).unwrap().command, Some(Command::Presets { action: None })));
        assert!(matches!(
            Cli::try_parse_from(["wolfgang", "settings", "set", "web-format", "avif"]).unwrap().command,
            Some(Command::Settings { action: Some(SettingsAction::Set { key: SettingKey::WebFormat, .. }) })
        ));
        assert!(matches!(
            Cli::try_parse_from(["wolfgang", "transcription", "install"]).unwrap().command,
            Some(Command::Transcription { action: Some(TranscriptionAction::Install) })
        ));
    }
}
