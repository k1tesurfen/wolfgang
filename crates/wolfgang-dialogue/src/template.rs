//! Tiny template syntax for Wolfgang's lines:
//! `{name}` inserts a value, `{name:singular:plural}` picks a German number form:
//! 1 → the singular as written (`ein Foto`), otherwise `<n> <plural>` (`42 Fotos`),
//! 0 → `keine <plural>`.

/// A placeholder value.
#[derive(Debug, Clone, PartialEq)]
pub enum Var {
    Num(u64),
    Text(String),
}

/// Renders `text`; unknown placeholders are left visible so mistakes get noticed.
pub fn render(text: &str, lookup: impl Fn(&str) -> Option<Var>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &rest[open + 1..open + close];
        let mut parts = inner.splitn(3, ':');
        let name = parts.next().unwrap_or_default();
        match (lookup(name), parts.next(), parts.next()) {
            (Some(Var::Num(n)), Some(one), Some(many)) => match n {
                0 => out.push_str(&format!("keine {many}")),
                1 => out.push_str(one),
                n => out.push_str(&format!("{n} {many}")),
            },
            (Some(Var::Num(n)), None, None) => out.push_str(&n.to_string()),
            (Some(Var::Text(t)), None, None) => out.push_str(&t),
            _ => out.push_str(&rest[open..=open + close]),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// Checks placeholders. `kind(name)` returns `Some(is_number)` for known names.
pub fn check(text: &str, kind: impl Fn(&str) -> Option<bool>) -> Result<(), String> {
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').ok_or_else(|| format!("'{{' ohne '}}' in „{text}“"))?;
        let inner = &rest[open + 1..open + close];
        let parts: Vec<&str> = inner.split(':').collect();
        match (kind(parts[0]), parts.len()) {
            (None, _) => return Err(format!("Platzhalter '{{{}}}' kenne ich nicht", parts[0])),
            (Some(false), n) if n > 1 => return Err(format!("'{{{inner}}}': Einzahl/Mehrzahl geht nur bei Zahlen")),
            (Some(_), 2) | (Some(_), 4..) => return Err(format!("'{{{inner}}}': erwartet {{name}} oder {{name:Einzahl:Mehrzahl}}")),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(name: &str) -> Option<Var> {
        match name {
            "images" => Some(Var::Num(42)),
            "fonts" => Some(Var::Num(1)),
            "none" => Some(Var::Num(0)),
            "folder" => Some(Var::Text("Strand".into())),
            _ => None,
        }
    }

    #[test]
    fn renders_values_and_plurals() {
        assert_eq!(render("{images:ein Foto:Fotos} und {fonts:eine Schrift:Schriften}", vars), "42 Fotos und eine Schrift");
        assert_eq!(render("{none:ein Foto:Fotos}", vars), "keine Fotos");
        assert_eq!(render("Ordner „{folder}“, {images}x", vars), "Ordner „Strand“, 42x");
        assert_eq!(render("{oops} bleibt {", vars), "{oops} bleibt {");
    }

    #[test]
    fn checks_placeholders() {
        let kind = |n: &str| vars(n).map(|v| matches!(v, Var::Num(_)));
        assert!(check("{images:ein Foto:Fotos} {folder}", kind).is_ok());
        assert!(check("{bilder}", kind).is_err());
        assert!(check("{folder:a:b}", kind).is_err());
        assert!(check("{images:nur eins}", kind).is_err());
        assert!(check("offen {images", kind).is_err());
    }
}
