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
    // Distinct content per path, or Wolfgang rightly treats them as duplicates.
    let shade = path.to_string_lossy().bytes().fold(7u8, |a, b| a.wrapping_mul(31).wrapping_add(b));
    let px = Pixels { width: 120, height: 80, rgba: vec![shade; 120 * 80 * 4], icc: None };
    std::fs::write(path, image::encode(&px, ImageFormat::Jpg, 80.0).unwrap()).unwrap();
}

/// Hotel/{Zimmer/1.jpg, Zimmer/2.jpg, Garten/3.jpg, Arial.ttf, preise.pdf}
fn hotel(t: &Path) -> PathBuf {
    let root = t.join("Hotel");
    jpg(&root.join("Zimmer/1.jpg"));
    jpg(&root.join("Zimmer/2.jpg"));
    jpg(&root.join("Garten/3.jpg"));
    if Path::new(ARIAL).exists() {
        std::fs::copy(ARIAL, root.join("Arial.ttf")).unwrap();
    }
    std::fs::write(root.join("preise.pdf"), "%PDF").unwrap();
    root
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
    assert!(summary.contains("Hotel-wolfgang"), "{summary}");
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
    assert!(said(&c).contains("Hotel-wolfgang"), "{}", said(&c));
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
    let settings = wolfgang_core::RunSettings { images: Some(ImageSettings::HighQuality { format: ImageFormat::Webp }), fonts: None };
    execute(&ws.plan(&settings, RerunPolicy::OnlyNew).unwrap(), &|_| {}, &AtomicBool::new(false)).unwrap();

    let (_, mut c) = talk(&[root], &UserStore::default());
    assert_eq!(node(&c), "rerun_all_known");
    assert!(said(&c).contains("„Alt-wolfgang“"), "{}", said(&c));
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
    std::fs::write(root.join("a.pdf"), "x").unwrap();
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
    let drops = [hotel_dir, std::mem::take(&mut with_keywords), empty];

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
        for _hop in 0..60 {
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
