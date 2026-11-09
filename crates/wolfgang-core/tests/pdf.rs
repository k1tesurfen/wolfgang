//! Pulling pictures out of PDFs, end to end. The PDFs are built here with lopdf,
//! so every kind of picture the extractor handles is covered.

use lopdf::{dictionary, Document, Object, Stream};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use wolfgang_core::image::{self, ColorTarget, Pixels};
use wolfgang_core::runlog::RunLog;
use wolfgang_core::settings::PdfSettings;
use wolfgang_core::*;

fn gradient(w: u32, h: u32) -> Pixels {
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 120, 255]);
        }
    }
    Pixels { width: w, height: h, rgba, icc: None }
}

fn image_dict(w: i64, h: i64, space: Object, bpc: i64) -> lopdf::Dictionary {
    dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h, "ColorSpace" => space, "BitsPerComponent" => bpc }
}

/// Page 1: a JPEG, RGB pixels with a soft mask, and a form holding a palette image.
/// Page 2: the same JPEG again (a logo on every page) and a 10×10 icon.
fn brochure(path: &Path) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let jpeg = image::encode(&gradient(200, 150), ImageFormat::Jpg, 90.0).unwrap();
    let mut d = image_dict(200, 150, "DeviceRGB".into(), 8);
    d.set("Filter", "DCTDecode");
    let photo = doc.add_object(Stream::new(d, jpeg.clone()));

    let (w, h) = (100, 80);
    let rgb: Vec<u8> = gradient(w, h).rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let alpha: Vec<u8> = (0..w * h).map(|i| if i % w < w / 2 { 0 } else { 255 }).collect();
    let mut mask = Stream::new(image_dict(w as i64, h as i64, "DeviceGray".into(), 8), alpha);
    mask.compress().unwrap();
    let mask = doc.add_object(mask);
    let mut d = image_dict(w as i64, h as i64, "DeviceRGB".into(), 8);
    d.set("SMask", mask);
    let mut raw = Stream::new(d, rgb);
    raw.compress().unwrap();
    let raw = doc.add_object(raw);

    let palette = Object::String(vec![255, 0, 0, 0, 0, 255], lopdf::StringFormat::Hexadecimal);
    let space = Object::Array(vec!["Indexed".into(), "DeviceRGB".into(), 1.into(), palette]);
    let indexed = doc.add_object(Stream::new(image_dict(64, 64, space, 8), (0..64 * 64).map(|i| (i / 64 % 2) as u8).collect()));
    let form = doc.add_object(Stream::new(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 64.into(), 64.into()],
                      "Resources" => dictionary! { "XObject" => dictionary! { "Ix" => indexed } } },
        b"/Ix Do".to_vec(),
    ));
    let icon = doc.add_object(Stream::new(image_dict(10, 10, "DeviceGray".into(), 8), vec![0; 100]));

    let pages_id = doc.new_object_id();
    let page = |doc: &mut Document, xobjects: lopdf::Dictionary| {
        let content = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
        doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                                     "Resources" => dictionary! { "XObject" => xobjects } })
    };
    let p1 = page(&mut doc, dictionary! { "Im1" => photo, "Im2" => raw, "Fm1" => form });
    let p2 = page(&mut doc, dictionary! { "Im1" => photo, "Ic" => icon });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![p1.into(), p2.into()], "Count" => 2,
                                          "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()] }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    doc.save(path).unwrap();
    jpeg
}

fn empty_pdf(path: &Path) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"BT ET".to_vec()));
    let p = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content });
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![p.into()], "Count" => 1 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

fn pdfs() -> RunSettings {
    RunSettings { pdfs: Some(PdfSettings {}), ..Default::default() }
}

#[test]
fn pictures_come_out_as_they_are() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Kunde");
    let jpeg = brochure(&dir.join("Broschüre Frühjahr.pdf"));
    empty_pdf(&dir.join("Nur Text.pdf"));

    let ws = Workspace::open(&[dir]).unwrap();
    assert_eq!(ws.summary().pdfs.total, 2);
    let plan = ws.plan(&pdfs(), RerunPolicy::OnlyNew).unwrap();
    let report = execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!((report.pdf_pictures, report.bytes_in, report.bytes_out), (3, 0, 0), "pictures aren't „savings“");

    let folder = plan.output_root.join("broschuere-fruehjahr");
    let mut files: Vec<String> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    assert_eq!(files, ["seite-01-1.jpg", "seite-01-2.png", "seite-01-3.png"], "no repeated logo, no icon");

    assert_eq!(std::fs::read(folder.join("seite-01-1.jpg")).unwrap(), jpeg, "JPEGs are copied byte for byte");
    let masked = image::decode(&folder.join("seite-01-2.png"), None, ColorTarget::Srgb).unwrap();
    assert_eq!((masked.width, masked.height), (100, 80));
    assert_eq!(masked.rgba[3], 0, "left half transparent from the soft mask");
    assert_eq!(masked.rgba[(99 * 4 + 3) as usize], 255);
    let indexed = image::decode(&folder.join("seite-01-3.png"), None, ColorTarget::Srgb).unwrap();
    assert_eq!(&indexed.rgba[..3], &[255, 0, 0], "palette colours, found inside a form");

    assert!(!plan.output_root.join("nur-text").exists(), "no empty folder for a PDF without pictures");
    assert_eq!(report.notes, vec![(std::path::PathBuf::from("Nur Text.pdf"), Note::PdfNoPictures)]);

    let log = RunLog::open(&plan.output_root).unwrap();
    let rec = log.files.values().find(|r| r.source.starts_with("Brosch")).unwrap();
    assert_eq!(rec.outputs.len(), 3, "every written picture is in the log: {:?}", rec.outputs);

    // Dropped again with „Alles nochmal“: the same folder is reused, not `-pdf`.
    let ws = Workspace::open(&[t.path().join("Kunde")]).unwrap();
    let again = ws.plan(&pdfs(), RerunPolicy::All).unwrap();
    assert!(again.jobs.iter().any(|j| j.target == Path::new("broschuere-fruehjahr")), "{:?}", again.jobs.iter().map(|j| &j.target).collect::<Vec<_>>());
}

#[test]
fn broken_pdf_fails_politely() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("K");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kaputt.pdf"), "%PDF-1.4 nope").unwrap();
    let ws = Workspace::open(&[dir]).unwrap();
    let plan = ws.plan(&pdfs(), RerunPolicy::OnlyNew).unwrap();
    let report = execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].1.contains("kaputt.pdf"), "{:?}", report.failed);
}
