//! Whole conversations with the real `dialogue/de.yaml` on real folders.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use wolfgang_core::image::{self, Pixels};
use wolfgang_core::settings::{ImageSettings, WebFormat};
use wolfgang_core::{execute, ImageFormat, RerunPolicy, UserStore, Workspace};
use wolfgang_dialogue::{builtin, Context, Conversation, Step};

const ARIAL: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";

fn jpg(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Clearly different content per file (JPEG would erase subtle differences),
    // or Wolfgang rightly treats them as duplicates.
    static NEXT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    let shade = NEXT.fetch_add(37, std::sync::atomic::Ordering::Relaxed);
    let px = Pixels { width: 120, height: 80, rgba: [shade, shade, shade, 255].repeat(120 * 80), icc: None };
    std::fs::write(path, image::encode(&px, ImageFormat::Jpg, 80.0).unwrap()).unwrap();
}

/// Hotel/{Zimmer/1.jpg, Zimmer/2.jpg, Garten/3.jpg, Arial.ttf, preise.docx}
fn hotel(t: &Path) -> PathBuf {
    let root = t.join("Hotel");
    jpg(&root.join("Zimmer/1.jpg"));
    jpg(&root.join("Zimmer/2.jpg"));
    jpg(&root.join("Garten/3.jpg"));
    if Path::new(ARIAL).exists() {
        std::fs::copy(ARIAL, root.join("Arial.ttf")).unwrap();
    }
    std::fs::write(root.join("preise.docx"), "PK").unwrap();
    root
}

/// A file with a video/audio extension. The conversation only needs the pile;
/// unreadable files count as "has sound" so Wolfgang still asks.
fn fake_media(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, path.to_string_lossy().as_bytes()).unwrap();
}

/// Videos and audio need the bundled ffmpeg (`scripts/build-ffmpeg.sh`); CI builds it.
fn media_tools() -> bool {
    let ok = wolfgang_core::media::tools().is_some();
    if !ok {
        eprintln!("skipped: no ffmpeg in target/ffmpeg (scripts/build-ffmpeg.sh)");
    }
    ok
}

fn talk(items: &[PathBuf], store: &UserStore) -> (Workspace, Conversation) {
    let ws = Workspace::open(items).unwrap();
    let conv = Conversation::new(builtin(), Context::new(&ws, store), 7);
    (ws, conv)
}

fn ask(step: &Step) -> (&str, &[String], Vec<&str>) {
    match step {
        Step::Ask { node, lines, answers, .. } => (node.as_str(), lines.as_slice(), answers.iter().map(|a| a.label.as_str()).collect()),
        other => panic!("expected a question, got {other:?}"),
    }
}

fn pick(conv: &mut Conversation, label_part: &str) {
    let (node, _, labels) = ask(conv.step());
    let i = labels.iter().position(|l| l.contains(label_part)).unwrap_or_else(|| panic!("no answer '{label_part}' in {node}: {labels:?}"));
    conv.choose(i);
}

fn node(conv: &Conversation) -> &str {
    ask(conv.step()).0
}

fn said(conv: &Conversation) -> String {
    ask(conv.step()).1.join(" | ")
}

#[test]
fn builtin_dialogue_is_valid() {
    let s = builtin();
    assert!(s.nodes.len() > 20);
    assert!(!s.idle.is_empty() && !s.working.is_empty());
}

#[test]
fn first_web_run_then_preset_then_like_last_time() {
    let t = tempfile::tempdir().unwrap();
    let root = hotel(t.path());
    let has_font = Path::new(ARIAL).exists();
    let mut store = UserStore::default();

    let (ws, mut c) = talk(std::slice::from_ref(&root), &store);
    assert_eq!(node(&c), "mode", "no history yet: straight to the mode question");
    let greeting = said(&c);
    assert!(greeting.contains("3 Fotos"), "{greeting}");
    assert!(greeting.contains("einer Datei kann ich nix") || greeting.contains("Eine Datei ist nicht"), "{greeting}");

    pick(&mut c, "Fürs Web");
    let Step::Ask { answers, .. } = c.step() else { panic!() };
    assert!(answers.iter().find(|a| a.label == "WebP").unwrap().current, "user default is marked");
    pick(&mut c, "AVIF");

    assert_eq!(node(&c), "web_size");
    c.type_answer("2.000px");
    assert_eq!(node(&c), "web_quality");
    c.type_answer("500");
    assert_eq!(node(&c), "web_quality", "invalid input keeps the question");
    assert!(said(&c).contains("von 1 bis 100") && said(&c).contains("„500“"), "{}", said(&c));
    c.type_answer("75");

    assert_eq!(node(&c), "prefix");
    c.type_answer("Hotel Seeblick");
    assert_eq!(node(&c), "keywords_ask");
    pick(&mut c, "jeden Ordner");
    assert!(said(&c).contains("„Garten“"), "{}", said(&c));
    c.type_answer("Gartenterrasse Blumen");
    assert!(said(&c).contains("„Zimmer“"), "{}", said(&c));
    pick(&mut c, "überspringen");

    assert_eq!(node(&c), "separator");
    let (_, _, labels) = ask(c.step());
    assert!(labels[0].contains("hotel-seeblick-gartenterrasse-blumen-0001.avif"), "{labels:?}");
    assert!(labels[2].contains("HotelSeeblickGartenterrasseBlumen0001.avif"), "{labels:?}");

    // Going back and forth keeps everything.
    c.back();
    assert_eq!(node(&c), "keywords_folder");
    pick(&mut c, "überspringen");
    pick(&mut c, "Bindestrich");
    pick(&mut c, "Nee");

    assert_eq!(node(&c), "confirm");
    let summary = said(&c);
    assert!(summary.contains("3 Fotos fürs Web als AVIF, 2000 px, Qualität 75"), "{summary}");
    assert!(summary.contains("Hotel/wolfgang_output"), "{summary}");
    assert_eq!(summary.contains("Schriften") || summary.contains("Schrift"), has_font, "{summary}");

    pick(&mut c, "Los");
    let Step::Start { settings, rerun, .. } = c.step().clone() else { panic!("{:?}", c.step()) };
    assert_eq!(rerun, RerunPolicy::OnlyNew);
    let Some(ImageSettings::Web(w)) = &settings.images else { panic!() };
    assert_eq!((w.format, w.max_side, w.quality), (WebFormat::Avif, 2000, 75));
    assert_eq!(w.naming.folder_keywords.get("Garten").map(String::as_str), Some("Gartenterrasse Blumen"));
    assert_eq!(settings.fonts.is_some(), has_font);

    let line = c.working_line();
    assert!(!line.is_empty() && !line.contains('{'));

    let report = execute(&ws.plan(&settings, rerun).unwrap(), &|_| {}, &AtomicBool::new(false)).unwrap();
    c.finished(&report);
    assert_eq!(node(&c), "done");
    assert!(said(&c).contains("Hotel/wolfgang_output"), "{}", said(&c));
    pick(&mut c, "Als Vorlage merken");
    c.type_answer("  Hotelbilder ");
    let Step::End { lines, .. } = c.step() else { panic!("{:?}", c.step()) };
    assert!(lines[0].contains("„Hotelbilder“"), "{lines:?}");
    c.remember(&mut store);
    assert!(store.last_run.is_some());
    assert!(store.preset("hotelbilder").is_some());

    // Second drop with one new picture: Wolfgang offers last time and the preset.
    jpg(&root.join("Garten/4.jpg"));
    let (_, mut c) = talk(&[root], &store);
    assert_eq!(node(&c), "how");
    let (_, _, labels) = ask(c.step());
    assert!(labels[0].starts_with("So wie letztes Mal (fürs Web als AVIF, 2000 px, Qualität 75, Name „Hotel Seeblick“)"), "{labels:?}");
    assert!(labels.contains(&"Vorlage „Hotelbilder“"), "{labels:?}");
    pick(&mut c, "So wie letztes Mal");
    assert_eq!(node(&c), "keywords_ask", "typed keywords aren't reused for new pictures");
    pick(&mut c, "Ohne");
    assert_eq!(node(&c), "rerun", "separator question skipped when using last time");
    assert!(said(&c).contains("sind neu") || said(&c).contains("eine ist neu"), "{}", said(&c));
    pick(&mut c, "Nur die neuen");
    assert!(said(&c).contains("das neue Foto"), "{}", said(&c));
    assert!(said(&c).contains("So wie letztes Mal"), "{}", said(&c));
}

#[test]
fn everything_known_is_said_up_front() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Alt");
    jpg(&root.join("a.jpg"));
    let ws = Workspace::open(std::slice::from_ref(&root)).unwrap();
    let settings = wolfgang_core::RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Webp }), ..Default::default() };
    execute(&ws.plan(&settings, RerunPolicy::OnlyNew).unwrap(), &|_| {}, &AtomicBool::new(false)).unwrap();

    let (_, mut c) = talk(&[root], &UserStore::default());
    assert_eq!(node(&c), "rerun_all_known");
    assert!(said(&c).contains("„Alt/wolfgang_output“"), "{}", said(&c));
    pick(&mut c, "Trotzdem");
    pick(&mut c, "Hochwertig");
    pick(&mut c, "JPG");
    assert_eq!(node(&c), "confirm", "not asked twice");
    pick(&mut c, "Los");
    assert!(matches!(c.step(), Step::Start { rerun: RerunPolicy::All, .. }));
}

#[test]
fn high_quality_is_short() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Scans");
    jpg(&root.join("a.jpg"));
    let (_, mut c) = talk(&[root], &UserStore::default());
    pick(&mut c, "Hochwertig");
    pick(&mut c, "PNG");
    assert_eq!(node(&c), "confirm");
    assert!(said(&c).contains("ein Foto hochwertig als PNG"), "{}", said(&c));
    pick(&mut c, "doch nicht");
    let Step::Cancelled { lines, .. } = c.step() else { panic!("{:?}", c.step()) };
    assert!(!lines.is_empty());
}

#[test]
fn fonts_only_need_no_questions() {
    if !Path::new(ARIAL).exists() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Fonts");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(ARIAL, root.join("Arial.ttf")).unwrap();
    let (_, c) = talk(&[root], &UserStore::default());
    assert_eq!(node(&c), "confirm");
    assert!(said(&c).contains("WOFF2"), "{}", said(&c));
    assert!(said(&c).contains("eine Schrift als WOFF2"), "{}", said(&c));
}

#[test]
fn nothing_useful_on_the_desk() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Docs");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.docx"), "x").unwrap();
    let (_, mut c) = talk(&[root], &UserStore::default());
    assert_eq!(node(&c), "nothing");
    c.choose(0);
    assert!(matches!(c.step(), Step::End { .. }));
}

#[test]
fn steps_serialize_for_the_app() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("X");
    jpg(&root.join("a.jpg"));
    let (_, c) = talk(&[root], &UserStore::default());
    let json = serde_json::to_value(c.step()).unwrap();
    assert_eq!(json["type"], "ask");
    assert_eq!(json["node"], "mode");
    assert_eq!(json["mood"], "thinking");
    assert!(json["answers"][0]["label"].as_str().unwrap().contains("Web"));
}

/// Plays many conversations with random answers on different drops and checks
/// Wolfgang never gets stuck or shows a raw `{placeholder}`.
#[test]
fn random_conversations_always_end_cleanly() {
    let t = tempfile::tempdir().unwrap();
    let hotel_dir = hotel(t.path());
    let mut with_keywords = t.path().join("Shop");
    jpg(&with_keywords.join("a.jpg"));
    jpg(&with_keywords.join("Schuhe/b.jpg"));
    std::fs::write(with_keywords.join("Schuhe/kw.txt"), "rote Schuhe").unwrap();
    let empty = t.path().join("Leer");
    std::fs::create_dir_all(&empty).unwrap();
    let mut drops = vec![hotel_dir, std::mem::take(&mut with_keywords), empty];
    if media_tools() {
        let mixed = t.path().join("Alles");
        jpg(&mixed.join("a.jpg"));
        fake_media(&mixed.join("Film/clip.mov"));
        fake_media(&mixed.join("Ton/interview.wav"));
        let films = t.path().join("Filme");
        fake_media(&films.join("a.mp4"));
        fake_media(&films.join("b.mkv"));
        drops.extend([mixed, films]);
    }

    let mut store = UserStore::default();
    let mut rng = fastrand::Rng::with_seed(1);
    let check = |text: &str, ctx: &str| {
        assert!(!text.contains('{') && !text.contains('}'), "unrendered placeholder in {ctx}: {text}");
        assert!(!text.contains("verheddert"), "dialogue got stuck in {ctx}");
    };

    for round in 0..300 {
        let drop = &drops[round % drops.len()];
        let ws = Workspace::open(std::slice::from_ref(drop)).unwrap();
        let mut c = Conversation::new(builtin(), Context::new(&ws, &store), round as u64);
        for _hop in 0..150 {
            match c.step().clone() {
                Step::Ask { node, lines, answers, input, can_go_back, .. } => {
                    for l in lines.iter().chain(answers.iter().map(|a| &a.label)) {
                        check(l, &node);
                    }
                    let roll = rng.usize(..10);
                    if can_go_back && roll == 0 {
                        c.back();
                    } else if let (Some(inp), true) = (&input, roll < 4 || answers.is_empty()) {
                        let typed = match inp.kind {
                            wolfgang_dialogue::script::InputKind::Number => ["0", "80", "1400", "abc", "99999"][rng.usize(..5)].to_owned(),
                            wolfgang_dialogue::script::InputKind::Text => ["Strand", "", "x".repeat(200).as_str(), "Müller & Söhne"][rng.usize(..4)].to_owned(),
                        };
                        c.type_answer(&typed);
                    } else {
                        assert!(!answers.is_empty(), "{node} has neither answers nor input");
                        c.choose(rng.usize(..answers.len()));
                    }
                }
                Step::Start { lines, settings, rerun, .. } => {
                    lines.iter().for_each(|l| check(l, "start"));
                    settings.validate().unwrap();
                    // Pretend the run happened (sometimes cancelled, sometimes with errors).
                    let plan = ws.plan(&settings, rerun).unwrap();
                    let report = wolfgang_core::Report {
                        output_root: plan.output_root.clone(),
                        converted: plan.jobs.len().saturating_sub(round % 2),
                        failed: if round % 3 == 0 { vec![("x.jpg".into(), "kaputt".into())] } else { vec![] },
                        bytes_in: 1_000_000,
                        bytes_out: 100_000,
                        cancelled: round % 5 == 0,
                        ..Default::default()
                    };
                    c.finished(&report);
                }
                Step::Cancelled { lines, .. } | Step::End { lines, .. } => {
                    lines.iter().for_each(|l| check(l, "end"));
                    c.remember(&mut store);
                    break;
                }
            }
        }
        assert!(matches!(c.step(), Step::End { .. } | Step::Cancelled { .. }), "round {round} didn't finish: {:?}", c.step());
    }
    assert!(!store.presets.is_empty() && store.last_run.is_some(), "the walk covered presets and last runs");
}

#[test]
fn a_video_pile() {
    if !media_tools() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Filme");
    fake_media(&root.join("Drohne Strand.MOV"));
    fake_media(&root.join("Team.mp4"));
    let (ws, mut c) = talk(&[root], &UserStore::default());
    assert!(said(&c).contains("Ich seh 2 Videos."), "{}", said(&c));
    assert_eq!(node(&c), "video_format", "no mode question for videos");
    let (_, _, labels) = ask(c.step());
    assert!(labels[0].starts_with("MP4"), "{labels:?}");
    pick(&mut c, "WebM");
    let Step::Ask { answers, .. } = c.step() else { panic!() };
    assert!(answers[1].current && answers[1].label.contains("1080p"), "Normal is the default: {answers:?}");
    c.type_answer("99");
    assert!(said(&c).contains("zwischen 144 und 4320"), "{}", said(&c));
    pick(&mut c, "Originalgröße");
    pick(&mut c, "Schöner");
    pick(&mut c, "Ohne Ton");
    assert!(said(&c).contains("2 Vorschaubilder"), "{}", said(&c));
    pick(&mut c, "Nee");
    assert_eq!(node(&c), "confirm");
    assert!(said(&c).contains("Also: 2 Videos als WebM, Originalgröße, schöner, ohne Ton."), "{}", said(&c));
    pick(&mut c, "Los");
    let Step::Start { settings, .. } = c.step().clone() else { panic!("{:?}", c.step()) };
    let v = settings.videos.clone().unwrap();
    assert_eq!((v.format, v.size, v.sound, v.poster), (wolfgang_core::VideoFormat::Webm, None, false, false));
    assert!(settings.images.is_none() && settings.audio.is_none());

    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();
    let targets: Vec<_> = plan.jobs.iter().map(|j| j.target.to_string_lossy().into_owned()).collect();
    assert_eq!(targets, ["drohne-strand.webm", "team.webm"], "web-safe original names, no numbers");
}

#[test]
fn an_audio_pile() {
    if !media_tools() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Podcast");
    fake_media(&root.join("Folge 1.wav"));
    let (_, mut c) = talk(&[root], &UserStore::default());
    assert!(said(&c).contains("Ich seh eine Audiodatei."), "{}", said(&c));
    pick(&mut c, "AAC");
    assert_eq!(node(&c), "audio_content");
    pick(&mut c, "Sprache");
    assert!(said(&c).contains("Also: eine Audiodatei als AAC (Sprache)."), "{}", said(&c));
}

#[test]
fn a_mixed_pile_goes_photos_videos_audio_and_is_remembered_per_pile() {
    if !media_tools() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let root = hotel(t.path());
    fake_media(&root.join("Film/rundgang.mov"));
    fake_media(&root.join("Ton/begruessung.m4a"));
    let mut store = UserStore::default();
    let (ws, mut c) = talk(std::slice::from_ref(&root), &store);
    assert!(said(&c).contains("3 Fotos, ein Video, eine Audiodatei und eine Schrift"), "{}", said(&c));
    pick(&mut c, "Hochwertig");
    pick(&mut c, "PNG");
    assert_eq!(node(&c), "video_format");
    assert!(said(&c).contains("ein Video"), "photos first, then: {}", said(&c));
    pick(&mut c, "MP4");
    pick(&mut c, "Klein");
    pick(&mut c, "Ausgewogen");
    pick(&mut c, "Mit Ton");
    pick(&mut c, "Ja");
    assert_eq!(node(&c), "audio_format");
    pick(&mut c, "MP3");
    pick(&mut c, "Musik, beste");
    assert_eq!(node(&c), "confirm");
    let text = said(&c);
    assert!(text.contains("3 Fotos hochwertig als PNG; ein Video als MP4, 720p, mit Ton und Vorschaubild; eine Audiodatei als MP3 (Musik in bester Qualität), dazu eine Schrift als WOFF2"), "{text}");

    // „Nochmal ändern“ walks through every pile again, with the answers marked.
    pick(&mut c, "Nochmal");
    assert_eq!(node(&c), "mode");
    pick(&mut c, "Hochwertig");
    let Step::Ask { answers, .. } = c.step() else { panic!() };
    assert!(answers.iter().any(|a| a.current && a.label.starts_with("PNG")), "{answers:?}");
    pick(&mut c, "PNG");
    pick(&mut c, "MP4");
    let Step::Ask { answers, .. } = c.step() else { panic!() };
    assert!(answers[0].current, "720p still marked: {answers:?}");
    for a in ["Klein", "Ausgewogen", "Mit Ton", "Ja", "MP3", "Sprache"] {
        pick(&mut c, a);
    }
    pick(&mut c, "Los");
    let Step::Start { settings, rerun, .. } = c.step().clone() else { panic!() };
    let plan = ws.plan(&settings, rerun).unwrap();
    let targets: Vec<_> = plan.jobs.iter().flat_map(|j| j.outputs().into_iter().map(|p| p.to_string_lossy().into_owned())).collect();
    assert!(targets.contains(&"Film/rundgang.mp4".to_owned()) && targets.contains(&"Film/rundgang.webp".to_owned()), "{targets:?}");
    assert!(targets.contains(&"Ton/begruessung.mp3".to_owned()), "{targets:?}");
    c.finished(&wolfgang_core::Report { output_root: plan.output_root.clone(), converted: 2, ..Default::default() });
    pick(&mut c, "Danke");
    c.remember(&mut store);

    // Next time, a drop with only a video: „letztes Mal“ talks about videos only.
    let films = t.path().join("Filme");
    fake_media(&films.join("neu.mp4"));
    let (_, mut c) = talk(&[films], &store);
    let (_, _, labels) = ask(c.step());
    assert!(labels[0].contains("So wie letztes Mal (als MP4, 720p, mit Ton und Vorschaubild)"), "{labels:?}");
    pick(&mut c, "letztes Mal");
    assert_eq!(node(&c), "confirm", "nothing left to ask");

    // And a drop with photos + audio picks up both remembered piles.
    let (_, c) = talk(&[root], &store);
    let (_, _, labels) = ask(c.step());
    assert!(labels[0].contains("Fotos hochwertig als PNG") && labels[0].contains("Audio als MP3 (Sprache)"), "{labels:?}");
}

#[test]
fn missing_media_tools_are_mentioned() {
    // Simulated by a summary; the real lookup finds the tools in the source tree.
    let mut s = wolfgang_core::Summary::default();
    s.videos.total = 2;
    s.media_tools_missing = true;
    let ctx = Context { summary: s, output_root: "/x/y-wolfgang".into(), store: UserStore::default(), keyword_words: Default::default(), sample_image: None };
    let c = Conversation::new(builtin(), ctx, 1);
    assert_eq!(node(&c), "nothing");
    assert!(said(&c).contains("fehlt mein Werkzeug"), "{}", said(&c));
}

fn val(yaml: &str) -> wolfgang_dialogue::script::Value {
    serde_norway::from_str(yaml).unwrap()
}

#[test]
fn flags_skip_answered_questions_but_keep_the_greeting() {
    let t = tempfile::tempdir().unwrap();
    let root = hotel(t.path());
    let store = UserStore {
        last_run: Some(wolfgang_core::RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Png }), ..Default::default() }),
        ..Default::default()
    };
    let ws = Workspace::open(std::slice::from_ref(&root)).unwrap();
    let fixed = vec![
        ("use_last".to_owned(), val("false")),
        ("mode".to_owned(), val("web")),
        ("web.format".to_owned(), val("avif")),
        ("web.max_side".to_owned(), val("2000")),
        ("naming.prefix".to_owned(), val("Hotel")),
    ];
    let mut c = Conversation::with_fixed(builtin(), Context::new(&ws, &store), 3, fixed);
    assert_eq!(node(&c), "web_quality", "how/mode/format/size were answered by flags");
    assert!(said(&c).contains("3 Fotos"), "greeting survives skipping: {}", said(&c));
    pick(&mut c, "Ausgewogen");
    assert_eq!(node(&c), "keywords_ask", "prefix answered by flag");
    pick(&mut c, "Ohne");
    pick(&mut c, "Bindestrich");
    pick(&mut c, "Nee");
    assert!(said(&c).contains("AVIF, 2000 px, Qualität 80, Name „Hotel“"), "{}", said(&c));
}

#[test]
fn flags_override_a_preset() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("P");
    jpg(&root.join("a.jpg"));
    let mut store = UserStore::default();
    let web = wolfgang_core::WebSettings { max_side: 800, ..Default::default() };
    store.save_preset("Klein", wolfgang_core::RunSettings { images: Some(ImageSettings::Web(web)), ..Default::default() }).unwrap();
    let ws = Workspace::open(std::slice::from_ref(&root)).unwrap();
    let fixed = vec![("preset".to_owned(), val("Klein")), ("web.quality".to_owned(), val("55"))];
    let mut c = Conversation::with_fixed(builtin(), Context::new(&ws, &store), 3, fixed);
    if node(&c) == "keywords_ask" {
        pick(&mut c, "Ohne");
    }
    assert_eq!(node(&c), "confirm");
    assert!(said(&c).contains("800 px, Qualität 55"), "{}", said(&c));
}

#[test]
fn pdfs_need_no_questions() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Kunde");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Broschüre.pdf"), "%PDF").unwrap();
    if Path::new(ARIAL).exists() {
        std::fs::copy(ARIAL, root.join("Arial.ttf")).unwrap();
    }
    let (_, c) = talk(&[root], &UserStore::default());
    assert_eq!(node(&c), "confirm", "straight to the summary");
    let text = said(&c);
    assert!(text.contains("Ich seh ein PDF") && text.contains("Bilder raus"), "{text}");
    assert!(text.contains("Also: die Bilder aus dem PDF"), "{text}");
}

#[test]
fn transparent_gifs_ask_for_a_background() {
    if !media_tools() {
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Gifs");
    std::fs::create_dir_all(&root).unwrap();
    let gif = concat!(env!("CARGO_MANIFEST_DIR"), "/../wolfgang-core/tests/fixtures/transparent-anim.gif");
    std::fs::copy(gif, root.join("katze.gif")).unwrap();
    let (_, mut c) = talk(&[root], &UserStore::default());
    let text = said(&c);
    assert!(text.contains("Ich seh ein animiertes GIF") && text.contains("GIFs mach ich zu Videos"), "{text}");
    pick(&mut c, "MP4");
    pick(&mut c, "Original");
    pick(&mut c, "Ausgewogen");
    assert_eq!(node(&c), "video_background", "no sound question for a GIF");
    pick(&mut c, "Schwarz");
    pick(&mut c, "Nee");
    assert!(said(&c).contains("ohne Ton, Durchsichtiges auf Schwarz"), "{}", said(&c));
}

#[test]
fn long_videos_come_with_an_estimate() {
    use wolfgang_core::media::{MediaInfo, VideoStream};
    use wolfgang_core::workspace::VideoFacts;
    let mut s = wolfgang_core::Summary::default();
    s.videos.total = 1;
    let clip = MediaInfo { duration: Some(600.0), video: Some(VideoStream { width: 3840, height: 2160, fps: Some(30.0), ..Default::default() }), audio: None };
    s.video_facts = vec![VideoFacts { info: Some(clip), known: false }];
    let mut store = UserStore::default();
    store.defaults.video_format = wolfgang_core::VideoFormat::Webm;
    let ctx = Context { summary: s, output_root: "/x/Filme-wolfgang".into(), store, keyword_words: Default::default(), sample_image: None };
    let mut c = Conversation::new(builtin(), ctx, 2);
    let Step::Ask { answers, .. } = c.step() else { panic!() };
    assert!(answers[1].current && answers[1].label.starts_with("WebM"), "the default from the settings is marked: {answers:?}");
    pick(&mut c, "WebM");
    pick(&mut c, "Normal");
    pick(&mut c, "Ausgewogen");
    pick(&mut c, "Ja");
    let text = said(&c);
    assert!(text.contains("etwa 9 Minuten") || text.contains("etwa 8 Minuten") || text.contains("etwa 10 Minuten"), "{text}");
    c.back();
    c.back();
    c.back();
    c.back();
    pick(&mut c, "MP4");
    for a in ["Normal", "Ausgewogen", "Ja"] {
        pick(&mut c, a);
    }
    assert!(said(&c).contains("etwa 2 Minuten"), "MP4 is much quicker (reading 4K is what takes time): {}", said(&c));
}
