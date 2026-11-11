//! Asking the web server: one request at a time, redirects are not followed
//! (each one is a hop of its own), optionally at a fixed address.

use reqwest::blocking::{Client, Response};
use reqwest::header::HeaderMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

/// What a browser on a Mac says, so firewalls treat Wolfgang like one.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
const TIMEOUT: Duration = Duration::from_secs(10);
/// Enough of a page to find its title and robots tag.
const BODY_LIMIT: usize = 2_000_000;

/// One answer from a web server.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Reply {
    pub status: u16,
    pub location: Option<String>,
    pub ip: Option<IpAddr>,
    pub ms: u64,
    pub server: Option<String>,
    /// `X-Redirect-By` (WordPress, Yoast SEO, Redirection, …).
    pub redirect_by: Option<String>,
    pub cloudflare: bool,
    pub x_robots: Option<String>,
    #[serde(skip)]
    pub body: String,
}

fn client(fixed: Option<(&str, IpAddr)>) -> reqwest::Result<Client> {
    // The app's updater uses rustls with ring as well; installing twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut b = Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(TIMEOUT).connect_timeout(Duration::from_secs(5)).user_agent(USER_AGENT).no_proxy();
    if let Some((host, ip)) = fixed {
        b = b.resolve(host, SocketAddr::new(ip, 0)); // the port comes from the URL
    }
    b.build()
}

/// GET `url` (at `ip`, if given), without following a redirect.
pub fn get(url: &url::Url, ip: Option<IpAddr>) -> Result<Reply, String> {
    let host = url.host_str().unwrap_or_default();
    let client = client(ip.map(|ip| (host, ip))).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let response = client.get(url.clone()).send().map_err(describe)?;
    let ms = started.elapsed().as_millis() as u64;
    Ok(read(response, ms))
}

fn read(r: Response, ms: u64) -> Reply {
    let h = r.headers();
    let text = |h: &HeaderMap, k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let mut reply = Reply {
        status: r.status().as_u16(),
        location: text(h, "location"),
        ip: r.remote_addr().map(|a| a.ip()),
        ms,
        server: text(h, "server"),
        redirect_by: text(h, "x-redirect-by"),
        cloudflare: h.contains_key("cf-ray"),
        x_robots: text(h, "x-robots-tag"),
        body: String::new(),
    };
    let html = text(h, "content-type").is_none_or(|t| t.contains("html"));
    if html {
        let mut buf = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(r, BODY_LIMIT as u64), &mut buf);
        reply.body = String::from_utf8_lossy(&buf).into_owned();
    }
    reply
}

/// reqwest's errors are nested; the innermost cause says what really happened.
fn describe(e: reqwest::Error) -> String {
    if e.is_timeout() {
        return "keine Antwort (Zeitüberschreitung)".into();
    }
    let mut source: &dyn std::error::Error = &e;
    while let Some(s) = source.source() {
        source = s;
    }
    let inner = source.to_string();
    let lower = inner.to_lowercase();
    if lower.contains("refused") {
        "Verbindung abgelehnt (da läuft kein Webserver)".into()
    } else if lower.contains("certificate") || lower.contains("tls") || lower.contains("handshake") {
        format!("HTTPS klappt nicht ({inner})")
    } else if lower.contains("unreachable") || lower.contains("no route") {
        "nicht erreichbar (keine Route)".into()
    } else if lower.contains("dns") || lower.contains("lookup") || lower.contains("nodename") {
        "Name nicht gefunden".into()
    } else {
        inner
    }
}

/// DNS over HTTPS (RFC 8484): the same packet, sent as a POST.
pub fn doh(url: &str, packet: &[u8]) -> std::io::Result<Vec<u8>> {
    let io = |e: reqwest::Error| std::io::Error::other(e.to_string());
    let response = client(None)
        .map_err(io)?
        .post(url)
        .header("content-type", "application/dns-message")
        .header("accept", "application/dns-message")
        .body(packet.to_vec())
        .send()
        .map_err(io)?;
    if !response.status().is_success() {
        return Err(std::io::Error::other(format!("HTTP {}", response.status())));
    }
    Ok(response.bytes().map_err(io)?.to_vec())
}

/// `<title>`, whitespace tidied, entities for the common characters decoded.
pub fn title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let end = lower[open_end..].find("</title")? + open_end;
    let t = html[open_end..end].split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.replace("&amp;", "&").replace("&#8211;", "–").replace("&ndash;", "–").replace("&#038;", "&").replace("&quot;", "\"").replace("&#039;", "'").replace("&nbsp;", " ");
    Some(t).filter(|t| !t.is_empty())
}

/// `<meta name="robots" content="noindex, …">` (also `googlebot`).
pub fn meta_noindex(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.match_indices("<meta").any(|(i, _)| {
        let tag = &lower[i..lower[i..].find('>').map_or(lower.len(), |e| i + e)];
        (tag.contains("name=\"robots\"") || tag.contains("name='robots'") || tag.contains("name=\"googlebot\"")) && tag.contains("noindex")
    })
}

pub fn is_wordpress(html: &str) -> bool {
    html.contains("/wp-content/") || html.contains("/wp-includes/") || html.to_ascii_lowercase().contains("content=\"wordpress")
}

/// WordPress's own page while it updates itself („Briefly unavailable for scheduled maintenance“).
pub fn wordpress_maintenance(status: u16, html: &str) -> bool {
    status == 503 && html.contains("scheduled maintenance")
}

/// `<meta http-equiv="refresh" content="0; url=…">`: a redirect the server doesn't know about.
pub fn meta_refresh(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let i = lower.find("http-equiv=\"refresh\"").or_else(|| lower.find("http-equiv='refresh'"))?;
    let tag_start = lower[..i].rfind('<')?;
    let tag = &html[tag_start..tag_start + lower[tag_start..].find('>')?];
    let url = tag.to_ascii_lowercase().find("url=")?;
    Some(tag[url + 4..].trim_matches(|c: char| c == '"' || c == '\'' || c.is_whitespace()).to_owned()).filter(|u| !u.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_page() {
        let html = r#"<html><head><meta charset="utf-8"><title>
            Startseite &#8211; Kunde &amp; Co</title>
            <meta name="robots" content="noindex, nofollow">
            <link rel="stylesheet" href="/wp-content/themes/x/style.css"></head></html>"#;
        assert_eq!(title(html).as_deref(), Some("Startseite – Kunde & Co"));
        assert!(meta_noindex(html) && is_wordpress(html));
        assert!(!meta_noindex(r#"<meta name="robots" content="index, follow"><meta name="description" content="noindex ist hier nur Text">"#));
        assert_eq!(meta_refresh(r#"<META HTTP-EQUIV="refresh" CONTENT="0; URL=https://neu.de/">"#).as_deref(), Some("https://neu.de/"));
        assert!(wordpress_maintenance(503, "<h1>Briefly unavailable for scheduled maintenance. Check back in a minute.</h1>"));
    }
}
