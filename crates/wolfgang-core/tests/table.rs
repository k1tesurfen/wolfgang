//! End-to-end for tables: a folder with workbooks and CSV files in, CSV and Excel out.

use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::Workbook;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use wolfgang_core::*;

fn workbook(path: &Path, sheets: &[(&str, &[&str])]) {
    let mut book = Workbook::new();
    for (name, rows) in sheets {
        let s = book.add_worksheet().set_name(*name).unwrap();
        for (r, row) in rows.iter().enumerate() {
            s.write_row(r as u32, 0, row.split(',')).unwrap();
        }
    }
    book.save(path).unwrap();
}

#[test]
fn tables_go_both_ways() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("Tabellen");
    std::fs::create_dir_all(&dir).unwrap();
    workbook(&dir.join("Preise 2024.xlsx"), &[("Einkauf", &["Artikel,Preis"]), ("Verkauf", &["Artikel,Preis"]), ("Tabelle3", &[])]);
    workbook(&dir.join("Kontakte.xlsx"), &[("Tabelle1", &["Name,Ort", "Anna,Köln"])]);
    workbook(&dir.join("Leer.xlsx"), &[("Tabelle1", &[])]);
    std::fs::write(dir.join("Bestellung.csv"), "PLZ;Menge\n01067;3\n").unwrap();

    let ws = Workspace::open(std::slice::from_ref(&dir)).unwrap();
    let s = ws.summary();
    assert_eq!((s.tables.total, s.csv_tables.total, s.excel_tables().total), (4, 1, 3));
    assert!(s.piles().tables && !s.piles().media());

    let settings = RunSettings { tables: Some(TableSettings {}), ..Default::default() };
    let plan = ws.plan(&settings, RerunPolicy::OnlyNew).unwrap();
    assert_eq!(plan.problems.len(), 1, "{:?}", plan.problems);
    assert!(plan.problems[0].0.ends_with("Leer.xlsx"));
    let report = execute(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!((report.tables, report.bytes_in), (3, 0), "a different format isn't „gespart“");

    let out = &plan.output_root;
    assert_eq!(std::fs::read_to_string(out.join("kontakte.csv")).unwrap(), "Name,Ort\nAnna,Köln\n");
    for sheet in ["einkauf", "verkauf"] {
        let csv = out.join(format!("preise-2024/preise-2024-{sheet}.csv"));
        assert_eq!(std::fs::read_to_string(&csv).unwrap(), "Artikel,Preis\n", "{}", csv.display());
    }
    assert!(!out.join("preise-2024/preise-2024-tabelle3.csv").exists(), "empty sheets are left out");

    let mut book: Xlsx<_> = open_workbook(out.join("bestellung.xlsx")).unwrap();
    let range = book.worksheet_range("Bestellung").unwrap();
    let row: Vec<_> = range.rows().nth(1).unwrap().to_vec();
    assert_eq!(row, [Data::String("01067".into()), Data::Float(3.0)], "the postcode keeps its zero");

    let again = Workspace::open(std::slice::from_ref(&dir)).unwrap().plan(&settings, RerunPolicy::OnlyNew).unwrap();
    assert!(again.jobs.is_empty(), "a second drop finds them done");
    let all = Workspace::open(std::slice::from_ref(&dir)).unwrap().plan(&settings, RerunPolicy::All).unwrap();
    let targets: Vec<_> = all.jobs.iter().map(|j| j.target.to_string_lossy().into_owned()).collect();
    assert_eq!(targets, ["bestellung.xlsx", "kontakte.csv", "preise-2024"], "doing them again overwrites its own files");
}
