//! SEO file names: `[PREFIX]-[KEYWORDS]-[ORIGINAL?]-[INDEX]`.

use serde::{Deserialize, Serialize};

/// Longest name (without index and extension) before keywords get trimmed.
const MAX_BASE_LEN: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Separator {
    /// `urlaub-strand-sonnenuntergang-0001`
    #[default]
    Dash,
    /// `urlaub_strand_sonnenuntergang_0001`
    Underscore,
    /// `UrlaubStrandSonnenuntergang0001`
    Caps,
}

/// Splits free text into filename-safe ASCII words.
/// German umlauts are transliterated (`ä` → `ae`), other accents stripped.
pub fn words(text: &str) -> Vec<String> {
    let mut ascii = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            'ä' => ascii.push_str("ae"),
            'ö' => ascii.push_str("oe"),
            'ü' => ascii.push_str("ue"),
            'Ä' => ascii.push_str("Ae"),
            'Ö' => ascii.push_str("Oe"),
            'Ü' => ascii.push_str("Ue"),
            'ß' => ascii.push_str("ss"),
            c if c.is_ascii() => ascii.push(c),
            c => ascii.push_str(deunicode::deunicode_char(c).unwrap_or(" ")),
        }
    }
    ascii
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Parses a keyword file: one keyword (phrase) per line, blank lines ignored.
pub fn keyword_words(text: &str) -> Vec<String> {
    text.lines().flat_map(words).collect()
}

/// Joins words in the given style. Dash and underscore names are lowercase.
pub fn join(words: &[String], sep: Separator) -> String {
    match sep {
        Separator::Dash => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("-"),
        Separator::Underscore => words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join("_"),
        Separator::Caps => words
            .iter()
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
            })
            .collect(),
    }
}

/// Builds the SEO base name (no extension).
///
/// Empty parts are skipped. If prefix, keywords and original name are all empty,
/// the original file name is used so the result is never just a number.
pub fn seo_name(prefix: &str, keywords: &[String], original_stem: Option<&str>, stem_fallback: &str, index: u32, sep: Separator) -> String {
    let prefix = words(prefix);
    let original = original_stem.map(words).unwrap_or_default();

    // Trim keywords (never prefix or original) so the name stays reasonable.
    let fixed_len: usize = prefix.iter().chain(&original).map(|w| w.len() + 1).sum();
    let mut kw = Vec::new();
    let mut len = fixed_len;
    for w in keywords {
        if len + w.len() + 1 > MAX_BASE_LEN {
            break;
        }
        len += w.len() + 1;
        kw.push(w.clone());
    }

    let mut all: Vec<String> = prefix.into_iter().chain(kw).chain(original).collect();
    if all.is_empty() {
        all = words(stem_fallback);
    }
    all.push(format!("{index:04}"));
    join(&all, sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn transliterates_german_and_accents() {
        assert_eq!(words("Größe Übergänge"), s(&["Groesse", "Uebergaenge"]));
        assert_eq!(words("Café crème, São Paulo!"), s(&["Cafe", "creme", "Sao", "Paulo"]));
        assert_eq!(words("  --  "), Vec::<String>::new());
    }

    #[test]
    fn keyword_file_lines_flatten_to_words() {
        assert_eq!(keyword_words("beach sunset\n\n  algarve \ngolden hour\n"), s(&["beach", "sunset", "algarve", "golden", "hour"]));
    }

    #[test]
    fn separator_styles() {
        let kw = s(&["beach", "sunset", "Algarve"]);
        assert_eq!(seo_name("Urlaub", &kw, None, "x", 1, Separator::Dash), "urlaub-beach-sunset-algarve-0001");
        assert_eq!(seo_name("Urlaub", &kw, None, "x", 12, Separator::Underscore), "urlaub_beach_sunset_algarve_0012");
        assert_eq!(seo_name("Urlaub", &kw, None, "x", 3, Separator::Caps), "UrlaubBeachSunsetAlgarve0003");
    }

    #[test]
    fn appends_original_name() {
        assert_eq!(seo_name("bild", &[], Some("DSC_4711"), "DSC_4711", 7, Separator::Dash), "bild-dsc-4711-0007");
    }

    #[test]
    fn falls_back_to_original_stem_when_everything_is_empty() {
        assert_eq!(seo_name("", &[], None, "IMG_0001 Kopie", 2, Separator::Dash), "img-0001-kopie-0002");
    }

    #[test]
    fn trims_overlong_keywords() {
        let kw: Vec<String> = (0..50).map(|i| format!("keyword{i}")).collect();
        let name = seo_name("p", &kw, None, "x", 1, Separator::Dash);
        assert!(name.len() <= MAX_BASE_LEN + 5, "{name}");
        assert!(name.starts_with("p-keyword0-keyword1"));
        assert!(name.ends_with("-0001"));
    }
}
