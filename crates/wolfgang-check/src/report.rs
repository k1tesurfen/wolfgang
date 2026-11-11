//! The printout: plain text in a monospace font, at most [`WIDTH`] characters
//! wide, so it reads the same in the app, in the terminal and pasted into an
//! email to the hoster.

use crate::{duration, Check, Level, Source};
use std::fmt::Write;

pub const WIDTH: usize = 72;

pub fn text(c: &Check, name: &str) -> String {
    let mut o = String::new();
    let rule = "─".repeat(WIDTH);
    let title = format!("{}S SEITEN-CHECK", name.to_uppercase());
    let _ = writeln!(o, "{title}{:>w$}", c.at, w = WIDTH - title.chars().count());
    let _ = writeln!(o, "{}", c.input);
    let _ = writeln!(o, "{rule}");

    section(&mut o, "ERGEBNIS");
    for f in &c.findings {
        let mark = match f.level {
            Level::Problem => "✗",
            Level::Odd => "!",
            Level::Ok => "✓",
        };
        wrap(&mut o, &format!("{mark} "), &f.text);
    }

    if !c.dns.views.is_empty() {
        section(&mut o, &format!("DNS  {}", c.host));
        if let Some(z) = &c.dns.zone {
            wrap(&mut o, "  ", &format!("Zone {z}, Nameserver: {}", if c.dns.nameservers.is_empty() { "–".into() } else { c.dns.nameservers.join(", ") }));
        }
        let mut last = None;
        for v in &c.dns.views {
            if last != Some(&v.source) {
                let _ = writeln!(
                    o,
                    "\n  {}",
                    match v.source {
                        Source::Own => "Eigene Nameserver der Domain (die Wahrheit):",
                        Source::Public => "Öffentliche Resolver (was die Welt sieht):",
                        Source::Mac => "Dieser Mac:",
                    }
                );
                last = Some(&v.source);
            }
            let what = if let Some(e) = &v.error {
                e.clone()
            } else if v.nxdomain {
                "gibt's nicht".into()
            } else if v.ips.is_empty() {
                "keine Adresse".into()
            } else {
                v.ips.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
            };
            let ttl = match (v.ttl, &v.source) {
                (Some(t), Source::Own) => format!("  (gültig {})", duration(t)),
                (Some(t), Source::Public) => format!("  (noch {})", duration(t)),
                _ => String::new(),
            };
            if v.source == Source::Mac {
                wrap(&mut o, "    ", &what);
            } else {
                let _ = writeln!(o, "    {}{ttl}", v.who);
                wrap(&mut o, "      ", &what);
            }
            if v.chain.len() > 1 {
                wrap(&mut o, "      ", &format!("über {}", v.chain.join(" → ")));
            }
        }
    }

    section(&mut o, "WEG DER ANFRAGE");
    for (i, h) in c.path.iter().enumerate() {
        let n = i + 1;
        match (&h.reply, &h.error) {
            (Some(r), _) => {
                let _ = writeln!(o, "  {n:>2}  {}  {:>6}", r.status, format!("{} ms", r.ms));
                wrap(&mut o, "      ", &h.url);
                if let Some(loc) = &r.location {
                    wrap(&mut o, "      → ", &format!("{loc}  (umgeleitet von {})", h.redirected_by.as_deref().unwrap_or("?")));
                }
                if let Some(ip) = r.ip {
                    let server = r.server.as_deref().map(|s| format!(", Server: {s}")).unwrap_or_default();
                    let _ = writeln!(o, "      an {ip}{server}{}", if r.cloudflare { ", über Cloudflare" } else { "" });
                }
            }
            (None, Some(e)) => {
                let _ = writeln!(o, "  {n:>2}  ✗");
                wrap(&mut o, "      ", &h.url);
                wrap(&mut o, "      ", e);
            }
            (None, None) => {}
        }
    }

    if !c.addresses.is_empty() {
        section(&mut o, "JEDE ADRESSE EINZELN");
        let mut last_host = None;
        for a in &c.addresses {
            if last_host != Some(&a.host) {
                let _ = writeln!(o, "  {}:", a.host);
                last_host = Some(&a.host);
            }
            let what = if a.untested {
                "nicht geprüft (kein IPv6 in diesem Netz)".into()
            } else if let Some(e) = &a.error {
                format!("✗ {e}")
            } else {
                let server = a.server.as_deref().map(|s| format!("  {s}")).unwrap_or_default();
                format!("{}  {} ms{server}", a.status.unwrap_or(0), a.ms.unwrap_or(0))
            };
            wrap(&mut o, "    ", &format!("{:<40}{what}", a.ip.to_string()));
        }
    }

    if let Some(p) = &c.page {
        section(&mut o, "DIE SEITE");
        wrap(&mut o, "  ", &p.url);
        let yes = |b: bool| if b { "ja" } else { "nein" };
        let _ = writeln!(o, "  Status:          {}", p.status);
        if let Some(t) = &p.title {
            wrap(&mut o, "  Titel:           ", t);
        }
        let _ = writeln!(o, "  WordPress:       {}", yes(p.wordpress));
        let _ = writeln!(o, "  Suchmaschinen:   {}", p.noindex.as_deref().map_or("erlaubt".to_owned(), |h| format!("ausgesperrt (noindex im {h})")));
        if p.maintenance {
            let _ = writeln!(o, "  Wartungsmodus:   ja");
        }
    }
    let _ = writeln!(o, "\n{rule}");
    o
}

fn section(o: &mut String, title: &str) {
    let _ = writeln!(o, "\n{title}");
}

/// Writes `text` after `prefix`, wrapped at [`WIDTH`]; continuation lines are indented to the prefix.
fn wrap(o: &mut String, prefix: &str, text: &str) {
    let indent = " ".repeat(prefix.chars().count());
    let room = WIDTH.saturating_sub(indent.len()).max(20);
    let mut line = String::new();
    let mut first = true;
    let mut flush = |line: &mut String, first: &mut bool| {
        let _ = writeln!(o, "{}{}", if *first { prefix } else { &indent }, line.trim_end());
        line.clear();
        *first = false;
    };
    for word in text.split(' ') {
        // Long URLs are cut hard; everything else wraps at spaces.
        let mut word = word.to_owned();
        while word.chars().count() > room {
            if !line.is_empty() {
                flush(&mut line, &mut first);
            }
            let head: String = word.chars().take(room).collect();
            word = word.chars().skip(room).collect();
            line.push_str(&head);
            flush(&mut line, &mut first);
        }
        if line.chars().count() + word.chars().count() + 1 > room && !line.is_empty() {
            flush(&mut line, &mut first);
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() || first {
        flush(&mut line, &mut first);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_the_page_edge() {
        let mut o = String::new();
        wrap(&mut o, "✓ ", &"wort ".repeat(30));
        assert!(o.lines().all(|l| l.chars().count() <= WIDTH), "{o}");
        assert!(o.lines().nth(1).unwrap().starts_with("  wort"), "continuation lines are indented: {o}");
        let mut o = String::new();
        wrap(&mut o, "      ", &format!("https://example.de/{}", "x".repeat(150)));
        assert!(o.lines().count() >= 3 && o.lines().all(|l| l.chars().count() <= WIDTH), "{o}");
    }
}
