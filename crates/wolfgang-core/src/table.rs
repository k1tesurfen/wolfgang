//! Tables: Excel (`.xlsx`, also old `.xls`, `.xlsm` and LibreOffice's `.ods`) to
//! CSV, one file per sheet, and CSV back to Excel (`.xlsx`).
//!
//! CSV out is the plain kind importers expect: UTF-8, commas, a dot in numbers,
//! dates as `2024-03-01`. CSV in is whatever Excel and friends write: commas,
//! semicolons or tabs, UTF-8 or Windows-1252 (German Excel's „CSV“).
//! Going to Excel, each cell's type is decided here instead of letting Excel
//! guess: only plain numbers become numbers, so `00123`, `1-2` or `2024-03-01`
//! stay exactly as they were written.

use crate::error::{io_err, Error, Result};
use calamine::{open_workbook_auto, Data, Range, Reader, Sheets, SheetType, SheetVisible};
use std::path::Path;

/// Excel's limits.
const MAX_ROWS: usize = 1_048_576;
const MAX_COLS: usize = 16_384;
/// Excel shows (and calculates with) 15 significant digits.
const DIGITS: usize = 15;

/// Which way a table goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableTask {
    /// These sheets (with something in them), each as its own CSV.
    ToCsv { sheets: Vec<String> },
    ToXlsx,
}

/// The visible sheets of a workbook that have something in them, in order.
pub fn sheets(path: &Path) -> Result<Vec<String>> {
    let mut book = open(path)?;
    let meta: Vec<_> = book.sheets_metadata().iter().filter(|s| s.typ == SheetType::WorkSheet).cloned().collect();
    let visible: Vec<_> = meta.iter().filter(|s| s.visible == SheetVisible::Visible).collect();
    // A workbook with only hidden sheets is odd, but then those are the content.
    let candidates = if visible.is_empty() { meta.iter().collect() } else { visible };
    let mut out = Vec::new();
    for s in candidates {
        let range = book.worksheet_range(&s.name).map_err(|e| unreadable(path, e))?;
        if range.used_cells().any(|(_, _, c)| !is_blank(c)) {
            out.push(s.name.clone());
        }
    }
    Ok(out)
}

/// One sheet as CSV.
pub fn sheet_to_csv(path: &Path, sheet: &str) -> Result<Vec<u8>> {
    let range = open(path)?.worksheet_range(sheet).map_err(|e| unreadable(path, e))?;
    csv_of(&range)
}

fn csv_of(range: &Range<Data>) -> Result<Vec<u8>> {
    let mut w = csv::WriterBuilder::new().terminator(csv::Terminator::Any(b'\n')).from_writer(Vec::new());
    // A sheet's data may start further in (B3); the CSV starts at A1 like Excel's own export.
    let (top, left) = range.start().map_or((0, 0), |(r, c)| (r as usize, c as usize));
    let width = left + range.width();
    let empty = vec![String::new(); width];
    for _ in 0..top {
        w.write_record(&empty).map_err(csv_err)?;
    }
    for row in range.rows() {
        let cells = std::iter::repeat_n(String::new(), left).chain(row.iter().map(cell_text));
        w.write_record(cells).map_err(csv_err)?;
    }
    w.into_inner().map_err(|e| Error::Table(e.to_string()))
}

/// A cell as Excel shows it, minus the local formatting.
fn cell_text(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => number_text(*f),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Data::Error(e) => e.to_string(),
        Data::DateTime(d) if d.is_duration() => {
            let secs = (d.as_f64() * 86_400.0).round() as i64;
            format!("{}{}:{:02}:{:02}", if secs < 0 { "-" } else { "" }, secs.abs() / 3600, secs.abs() / 60 % 60, secs.abs() % 60)
        }
        Data::DateTime(d) => {
            let (y, mo, day, h, mi, s, _) = d.to_ymd_hms_milli();
            let has_time = d.as_f64().fract() != 0.0;
            match (d.as_f64() < 1.0, has_time) {
                (true, _) => format!("{h:02}:{mi:02}:{s:02}"),
                (false, false) => format!("{y:04}-{mo:02}-{day:02}"),
                (false, true) => format!("{y:04}-{mo:02}-{day:02} {h:02}:{mi:02}:{s:02}"),
            }
        }
    }
}

/// `0.30000000000000004` → `0.3`, like Excel shows it; never `1e20`.
fn number_text(f: f64) -> String {
    let rounded: f64 = format!("{:.*e}", DIGITS - 1, f).parse().unwrap_or(f);
    rounded.to_string()
}

fn is_blank(c: &Data) -> bool {
    match c {
        Data::Empty => true,
        Data::String(s) => s.is_empty(),
        _ => false,
    }
}

/// A CSV file as an Excel workbook with one sheet, named like the file.
pub fn csv_to_xlsx(path: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    let text = decode(&bytes);
    let delimiter = sniff_delimiter(&text);
    let decimal = if delimiter == b';' { ',' } else { '.' };
    let mut reader = csv::ReaderBuilder::new().delimiter(delimiter).has_headers(false).flexible(true).from_reader(text.as_bytes());

    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    sheet.set_name(sheet_name(&stem)).map_err(xlsx_err)?;
    for (r, record) in reader.records().enumerate() {
        let record = record.map_err(|e| Error::Table(format!("Die CSV ist kaputt ({e})")))?;
        if r >= MAX_ROWS || record.len() > MAX_COLS {
            return Err(Error::Table("Die Tabelle ist größer, als Excel erlaubt".into()));
        }
        for (c, field) in record.iter().enumerate() {
            let (row, col) = (r as u32, c as u16);
            match number(field, decimal) {
                Some(n) => sheet.write_number(row, col, n).map(drop),
                None if field.is_empty() => Ok(()),
                None => sheet.write_string(row, col, field).map(drop),
            }
            .map_err(xlsx_err)?;
        }
    }
    sheet.autofit();
    book.save_to_buffer().map_err(xlsx_err)
}

/// UTF-8 (with or without BOM), else Windows-1252: what Excel writes on a Mac or PC.
fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(),
    }
}

/// Comma, semicolon or tab: whichever the first line has most of (outside quotes).
fn sniff_delimiter(text: &str) -> u8 {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut counts = [(b',', 0), (b';', 0), (b'\t', 0)];
    let mut quoted = false;
    for b in line.bytes() {
        match b {
            b'"' => quoted = !quoted,
            _ if quoted => {}
            _ => {
                if let Some(c) = counts.iter_mut().find(|(d, _)| *d == b) {
                    c.1 += 1;
                }
            }
        }
    }
    // Ties go to the comma (first in the list).
    counts.iter().fold((b',', 0), |best, &c| if c.1 > best.1 { c } else { best }).0
}

/// A plain number that comes back exactly as written: `-12`, `3.5` (`3,5` in
/// semicolon files). Leading zeros, thousands separators, signs, exponents, dates
/// and anything longer than Excel can hold stay text.
fn number(field: &str, decimal: char) -> Option<f64> {
    let (int, frac) = match field.strip_prefix('-').unwrap_or(field).split_once(decimal) {
        Some((i, f)) => (i, Some(f)),
        None => (field.strip_prefix('-').unwrap_or(field), None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let plain = digits(int) && (int == "0" || !int.starts_with('0')) && frac.is_none_or(digits);
    let significant = int.trim_start_matches('0').len() + frac.map_or(0, str::len);
    if !plain || significant > DIGITS {
        return None;
    }
    field.replacen(decimal, ".", 1).parse().ok()
}

/// Sheet names: at most 31 characters, none of `[]:*?/\`, no apostrophe at the ends.
fn sheet_name(stem: &str) -> String {
    let clean: String = stem.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
    let clean = clean.trim().trim_matches('\'').trim();
    if clean.is_empty() || clean.eq_ignore_ascii_case("history") {
        "Tabelle1".into()
    } else {
        clean.to_owned()
    }
}

fn open(path: &Path) -> Result<Sheets<std::io::BufReader<std::fs::File>>> {
    open_workbook_auto(path).map_err(|e| unreadable(path, e))
}

fn unreadable(path: &Path, e: impl std::fmt::Display) -> Error {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Error::Table(format!("Kann {name} nicht als Tabelle lesen – vielleicht mit Passwort geschützt? ({e})"))
}

fn csv_err(e: csv::Error) -> Error {
    Error::Table(e.to_string())
}

fn xlsx_err(e: rust_xlsxwriter::XlsxError) -> Error {
    Error::Encode { format: "Excel", reason: e.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook, Xlsx};
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
    use std::io::Write;

    fn xlsx(dir: &Path, build: impl FnOnce(&mut Workbook)) -> std::path::PathBuf {
        let mut book = Workbook::new();
        build(&mut book);
        let path = dir.join("Preisliste.xlsx");
        book.save(&path).unwrap();
        path
    }

    #[test]
    fn excel_becomes_plain_csv() {
        let t = tempfile::tempdir().unwrap();
        let path = xlsx(t.path(), |b| {
            let date = Format::new().set_num_format("dd.mm.yyyy");
            let s = b.add_worksheet().set_name("Preise 2024").unwrap();
            s.write_row(0, 0, ["Artikel", "Preis", "Ab", "Lager"]).unwrap();
            s.write_string(1, 0, "Stuhl, gepolstert").unwrap();
            s.write_number(1, 1, 0.1 + 0.2).unwrap();
            s.write_datetime_with_format(1, 2, ExcelDateTime::from_ymd(2024, 3, 1).unwrap(), &date).unwrap();
            s.write_boolean(1, 3, true).unwrap();
            s.write_string(2, 0, "Tisch „Eiche“").unwrap();
            s.write_number(2, 1, 1250.0).unwrap();
            b.add_worksheet().set_name("Leer").unwrap();
            b.add_worksheet().set_name("Versteckt").unwrap().write_string(0, 0, "geheim").unwrap().set_hidden(true);
            b.add_worksheet().set_name("Notizen").unwrap().write_string(2, 1, "später").unwrap();
        });
        assert_eq!(sheets(&path).unwrap(), ["Preise 2024", "Notizen"], "empty and hidden sheets are left out");
        let csv = String::from_utf8(sheet_to_csv(&path, "Preise 2024").unwrap()).unwrap();
        assert_eq!(csv, "Artikel,Preis,Ab,Lager\n\"Stuhl, gepolstert\",0.3,2024-03-01,TRUE\nTisch „Eiche“,1250,,\n");
        let notes = String::from_utf8(sheet_to_csv(&path, "Notizen").unwrap()).unwrap();
        assert_eq!(notes, ",\n,\n,später\n", "cells keep their place");
    }

    #[test]
    fn csv_becomes_excel_without_guessing() {
        let t = tempfile::tempdir().unwrap();
        // German Excel: semicolons, decimal comma, Windows-1252.
        let (text, _, _) = encoding_rs::WINDOWS_1252.encode("PLZ;Preis;Größe;Datum;Formel\n01067;3,50;1-2;01.03.2024;=1+1\n80331;-12;1.000;;\n");
        let path = t.path().join("Bestellung März.csv");
        std::fs::write(&path, &text).unwrap();
        let out = t.path().join("out.xlsx");
        std::fs::write(&out, csv_to_xlsx(&path).unwrap()).unwrap();

        let mut book: Xlsx<_> = open_workbook(&out).unwrap();
        assert_eq!(book.sheet_names(), ["Bestellung März"]);
        let r = book.worksheet_range("Bestellung März").unwrap();
        let row = |n: usize| r.rows().nth(n).unwrap().to_vec();
        let s = |v: &str| Data::String(v.into());
        assert_eq!(row(0), [s("PLZ"), s("Preis"), s("Größe"), s("Datum"), s("Formel")]);
        assert_eq!(row(1), [s("01067"), Data::Float(3.5), s("1-2"), s("01.03.2024"), s("=1+1")]);
        assert_eq!(row(2)[..3], [Data::Float(80331.0), Data::Float(-12.0), s("1.000")], "a dot in a semicolon file isn't a decimal point");
    }

    #[test]
    fn numbers_only_when_they_come_back_unchanged() {
        assert_eq!(number("42", '.'), Some(42.0));
        assert_eq!(number("-0.5", '.'), Some(-0.5));
        assert_eq!(number("0,25", ','), Some(0.25));
        for text in ["", "-", "007", "1.5", "1e5", "+3", "1 000", ".5", "5.", "12345678901234567", "0x1F", "NaN"] {
            assert_eq!(number(text, ','), None, "{text}");
        }
        assert_eq!(number("1.5", '.'), Some(1.5));
        assert_eq!(number("1,5", '.'), None);
    }

    #[test]
    fn finds_the_delimiter() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3"), b';');
        assert_eq!(sniff_delimiter("\n\"x;y\",b,c"), b',');
        assert_eq!(sniff_delimiter("a\tb"), b'\t');
        assert_eq!(sniff_delimiter("nur eine spalte"), b',');
    }

    #[test]
    fn sheet_names_follow_excels_rules() {
        assert_eq!(sheet_name("Umsatz [Q1]: 2024/25"), "Umsatz Q1 202425");
        assert_eq!(sheet_name(&"x".repeat(40)).len(), 31);
        assert_eq!(sheet_name("'''"), "Tabelle1");
    }

    /// LibreOffice's format: a zip with the cells in `content.xml`.
    #[test]
    fn libreoffice_tables_too() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("Mitglieder.ods");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/vnd.oasis.opendocument.spreadsheet").unwrap();
        zip.start_file("META-INF/manifest.xml", stored).unwrap();
        zip.write_all(br#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#).unwrap();
        zip.start_file("content.xml", stored).unwrap();
        zip.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
<office:body><office:spreadsheet><table:table table:name="Verein">
<table:table-row><table:table-cell office:value-type="string"><text:p>Name</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Beitrag</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell office:value-type="string"><text:p>Jürgen</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="12.5"><text:p>12,50</text:p></table:table-cell></table:table-row>
</table:table></office:spreadsheet></office:body></office:document-content>"#
                .as_bytes(),
        )
        .unwrap();
        zip.finish().unwrap();
        assert_eq!(sheets(&path).unwrap(), ["Verein"]);
        assert_eq!(String::from_utf8(sheet_to_csv(&path, "Verein").unwrap()).unwrap(), "Name,Beitrag\nJürgen,12.5\n");
    }

    #[test]
    fn rejects_files_that_arent_excel() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("kaputt.xlsx");
        std::fs::write(&path, "nope").unwrap();
        assert!(sheets(&path).unwrap_err().to_string().contains("kaputt.xlsx"));
    }
}
