//! „Guck mal, die Seite“: a pasted URL, checked from the outside. Is it the
//! DNS, the server or WordPress?
//!
//! 1. DNS: the domain's own nameservers (the truth) against public resolvers and
//!    this Mac — differences mean a change hasn't arrived everywhere yet.
//! 2. The path: one request per hop, until the final page, with who redirected.
//! 3. Every address of the domain asked on its own (old IPv6 records, one server
//!    of several out of date).
//! 4. The final page: title, WordPress, maintenance mode, noindex.
//!
//! Then findings with a level, and a verdict. [`report::text`] prints it all.

pub mod dns;
pub mod report;
pub mod web;

use dns::{Server, A, AAAA, NS, SOA};
use serde::Serialize;
use std::collections::BTreeSet;
use std::net::{IpAddr, ToSocketAddrs, UdpSocket};
use url::Url;

/// At most this many redirects are followed.
const MAX_HOPS: usize = 15;
/// Slower than this, a page is worth mentioning.
const SLOW_MS: u64 = 3000;
/// At most this many of the domain's own nameservers are asked.
const MAX_NAMESERVERS: usize = 4;

/// The public resolvers asked, with their DNS-over-HTTPS address for networks that block port 53.
const PUBLIC: [(&str, &str, &str); 3] = [
    ("Cloudflare", "1.1.1.1", "https://cloudflare-dns.com/dns-query"),
    ("Google", "8.8.8.8", "https://dns.google/dns-query"),
    ("Quad9", "9.9.9.9", "https://dns.quad9.net/dns-query"),
];

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// As pasted.
    pub input: String,
    pub url: String,
    pub host: String,
    pub at: String,
    pub dns: Dns,
    pub path: Vec<Hop>,
    pub addresses: Vec<Address>,
    pub page: Option<Page>,
    pub findings: Vec<Finding>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Dns {
    /// The zone the host belongs to (`example.de` for `www.example.de`).
    pub zone: Option<String>,
    pub nameservers: Vec<String>,
    pub views: Vec<View>,
    /// Port 53 is closed on this network: public resolvers were asked over HTTPS,
    /// the domain's own nameservers couldn't be asked.
    pub port53_blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// One of the domain's own nameservers.
    Own,
    Public,
    /// This Mac's resolver (with `/etc/hosts` and the local cache).
    Mac,
}

/// What one DNS server says about the host.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    pub source: Source,
    /// „ns1.hoster.de“, „Cloudflare (1.1.1.1)“, „Dieser Mac“.
    pub who: String,
    /// `www.example.de → example.de`; empty without CNAMEs.
    pub chain: Vec<String>,
    pub ips: Vec<IpAddr>,
    /// Own nameservers: how long resolvers may keep the addresses; public: how
    /// long they still will.
    pub ttl: Option<u32>,
    pub nxdomain: bool,
    pub error: Option<String>,
}

impl View {
    fn new(source: Source, who: String) -> Self {
        View { source, who, chain: Vec::new(), ips: Vec::new(), ttl: None, nxdomain: false, error: None }
    }

    fn answered(&self) -> bool {
        self.error.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Hop {
    pub url: String,
    pub reply: Option<web::Reply>,
    pub error: Option<String>,
    /// Who sent the redirect: „WordPress“, „Yoast SEO“, „Cloudflare“, „Webserver (nginx)“.
    pub redirected_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Address {
    /// The name asked for: the pasted host, or the one the path ends on (`www.…`).
    pub host: String,
    pub ip: IpAddr,
    pub status: Option<u16>,
    pub ms: Option<u64>,
    pub server: Option<String>,
    pub error: Option<String>,
    /// IPv6 can't be tried from this Mac (no IPv6 on this network).
    pub untested: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub url: String,
    pub status: u16,
    pub title: Option<String>,
    pub wordpress: bool,
    /// Where `noindex` was found: „Meta-Tag“ or „X-Robots-Tag“.
    pub noindex: Option<String>,
    pub maintenance: bool,
    pub meta_refresh: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Ok,
    Odd,
    Problem,
}

/// Where a finding points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Dns,
    Server,
    WordPress,
    Page,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub level: Level,
    pub area: Area,
    pub text: String,
}

/// The one-word answer, also the key for Wolfgang's line in the dialogue file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Nothing wrong.
    Fine,
    /// Works, but something's odd.
    Odd,
    /// A problem, most likely in the DNS …
    Dns,
    /// … on the server / at the hoster …
    Server,
    /// … or in WordPress.
    WordPress,
}

impl Verdict {
    pub fn key(self) -> &'static str {
        match self {
            Self::Fine => "fine",
            Self::Odd => "odd",
            Self::Dns => "dns",
            Self::Server => "server",
            Self::WordPress => "wordpress",
        }
    }
}

impl Check {
    /// The most important finding (for the speech bubble).
    pub fn headline(&self) -> Option<&Finding> {
        self.findings.first().filter(|f| f.level > Level::Ok)
    }
}

/// What Wolfgang is doing right now (for the app's bubble).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Dns,
    Path,
    Addresses,
}

/// `example.de`, `www.example.de/kontakt`, `https://…`: a URL to start from.
pub fn parse(input: &str) -> Result<Url, String> {
    let t = input.trim();
    let with_scheme = if t.contains("://") { t.to_owned() } else { format!("http://{t}") };
    let url = Url::parse(&with_scheme).map_err(|_| format!("„{t}“ ist keine Adresse, mit der ich was anfangen kann"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("„{t}“ ist keine Webadresse"));
    }
    match url.host_str() {
        Some(h) if h.contains('.') || h.parse::<IpAddr>().is_ok() || h.starts_with('[') => Ok(url),
        _ => Err(format!("„{t}“ ist keine Adresse, mit der ich was anfangen kann")),
    }
}

/// The whole check. Takes a few seconds; every part has its own timeout.
pub fn check(input: &str, stage: &(dyn Fn(Stage) + Sync)) -> Result<Check, String> {
    let url = parse(input)?;
    let host = url.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']').to_lowercase();
    let guessed_scheme = !input.contains("://");

    stage(Stage::Dns);
    let dns = if host.parse::<IpAddr>().is_ok() { Dns::default() } else { look_up(&host) };

    stage(Stage::Path);
    let path = follow(&url, guessed_scheme);

    stage(Stage::Addresses);
    // The last URL on the pasted host, asked at each of its addresses; and the
    // same for the host the path ends on, if that's another one (`www.…`).
    let host_of = |u: &Url| u.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']').to_lowercase();
    let urls: Vec<Url> = path.iter().filter_map(|h| Url::parse(&h.url).ok()).collect();
    let mut addresses = Vec::new();
    if let Some(u) = urls.iter().rev().find(|u| host_of(u) == host) {
        addresses.extend(each_address(u, &host, &dns_addresses(&dns, &host)));
    }
    if let Some(end) = urls.last().filter(|u| host_of(u) != host && path.last().is_some_and(|h| h.reply.is_some())) {
        let end_host = host_of(end);
        let ips = match end_host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => public_view(&end_host, PUBLIC[0].0, PUBLIC[0].1, PUBLIC[0].2).0.ips,
        };
        addresses.extend(each_address(end, &end_host, &ips));
    }

    let page = path.last().and_then(|h| {
        let r = h.reply.as_ref()?;
        let noindex = if r.x_robots.as_deref().is_some_and(|x| x.to_lowercase().contains("noindex")) {
            Some("X-Robots-Tag".to_owned())
        } else {
            web::meta_noindex(&r.body).then(|| "Meta-Tag".to_owned())
        };
        Some(Page {
            url: h.url.clone(),
            status: r.status,
            title: web::title(&r.body),
            wordpress: web::is_wordpress(&r.body) || h.redirected_by.as_deref() == Some("WordPress"),
            noindex,
            maintenance: web::wordpress_maintenance(r.status, &r.body),
            meta_refresh: web::meta_refresh(&r.body),
        })
    });

    let mut c = Check {
        input: input.trim().to_owned(),
        url: url.to_string(),
        host,
        at: chrono::Local::now().format("%d.%m.%Y, %H:%M").to_string(),
        dns,
        path,
        addresses,
        page,
        findings: Vec::new(),
        verdict: Verdict::Fine,
    };
    c.findings = findings(&c);
    c.verdict = verdict(&c.findings);
    Ok(c)
}

// ── DNS ──────────────────────────────────────────────────────────────────────

fn look_up(host: &str) -> Dns {
    let mut d = Dns::default();
    // Public resolvers in parallel; over HTTPS if port 53 is closed.
    let public: Vec<(View, bool)> = std::thread::scope(|s| {
        let handles: Vec<_> = PUBLIC.iter().map(|&(name, ip, doh)| s.spawn(move || public_view(host, name, ip, doh))).collect();
        handles.into_iter().map(|h| h.join().expect("no panics")).collect()
    });
    d.port53_blocked = public.iter().all(|(_, udp_ok)| !udp_ok) && public.iter().any(|(v, _)| v.answered());
    let resolver = public.iter().find(|(v, _)| v.answered()).map(|(_, udp)| if *udp { Server::Udp(PUBLIC[0].1.parse().unwrap()) } else { Server::Https(PUBLIC[0].2) });
    let public: Vec<View> = public.into_iter().map(|(v, _)| v).collect();

    let mut own = Vec::new();
    if let Some(resolver) = &resolver {
        if let Some(zone) = zone_of(resolver, host) {
            let ns = nameservers(resolver, &zone);
            d.nameservers = ns.iter().map(|(n, _)| n.clone()).collect();
            d.zone = Some(zone);
            if d.port53_blocked {
                let mut v = View::new(Source::Own, "Eigene Nameserver".into());
                v.error = Some("kann ich von hier nicht fragen (Port 53 ist in diesem Netz zu)".into());
                own.push(v);
            } else {
                own = std::thread::scope(|s| {
                    let handles: Vec<_> = ns.iter().take(MAX_NAMESERVERS).map(|(name, ip)| s.spawn(move || own_view(resolver, host, name, *ip))).collect();
                    handles.into_iter().map(|h| h.join().expect("no panics")).collect()
                });
            }
        }
    }
    d.views = own;
    d.views.extend(public);
    d.views.push(mac_view(host));
    d
}

/// A public resolver's view, and whether plain DNS (port 53) worked.
fn public_view(host: &str, name: &str, ip: &str, doh: &'static str) -> (View, bool) {
    let mut v = View::new(Source::Public, format!("{name} ({ip})"));
    let udp = Server::Udp(ip.parse().expect("constant"));
    let (server, udp_ok) = match dns::query(&udp, host, A, true) {
        Ok(_) => (udp, true),
        Err(_) => (Server::Https(doh), false),
    };
    fill(&mut v, &server, host, true);
    (v, udp_ok)
}

/// A and AAAA of `host` from `server`, following CNAMEs inside the answer.
fn fill(v: &mut View, server: &Server, host: &str, recursive: bool) {
    let mut ips = BTreeSet::new();
    for kind in [A, AAAA] {
        match dns::query(server, host, kind, recursive) {
            Ok(a) => {
                v.nxdomain |= a.rcode == dns::NXDOMAIN;
                if a.rcode != 0 && a.rcode != dns::NXDOMAIN {
                    v.error = Some(rcode_text(a.rcode));
                }
                if kind == A {
                    v.chain = chain(&a, host);
                }
                let last = v.chain.last().cloned().unwrap_or_else(|| host.to_owned());
                ips.extend(a.answers.iter().filter(|r| r.name == last).filter_map(|r| if let dns::Data::Ip(ip) = r.data { Some(ip) } else { None }));
                if let Some(t) = a.address_ttl() {
                    v.ttl = Some(v.ttl.map_or(t, |old| old.min(t)));
                }
            }
            Err(e) => v.error = Some(io_text(&e)),
        }
    }
    v.ips = ips.into_iter().collect();
}

/// `[www.example.de, example.de]` if the host is a CNAME; empty otherwise.
fn chain(a: &dns::Answer, host: &str) -> Vec<String> {
    let mut chain = vec![host.to_owned()];
    while let Some(next) = a.cname_of(chain.last().unwrap()) {
        if chain.contains(&next) || chain.len() > 8 {
            break;
        }
        chain.push(next);
    }
    if chain.len() == 1 {
        chain.clear();
    }
    chain
}

/// The zone a name belongs to: where its SOA is. A bare top-level domain
/// (`de`) means the domain itself isn't registered.
fn zone_of(resolver: &Server, name: &str) -> Option<String> {
    let a = dns::query(resolver, name, SOA, true).ok()?;
    a.answers.iter().chain(&a.authority).find(|r| r.kind == SOA).map(|r| r.name.clone()).filter(|z| z.contains('.'))
}

/// The zone's nameservers with an address each.
fn nameservers(resolver: &Server, zone: &str) -> Vec<(String, IpAddr)> {
    let mut names = dns::query(resolver, zone, NS, true).map(|a| a.names(NS)).unwrap_or_default();
    names.sort();
    names.dedup();
    names.into_iter().filter_map(|n| dns::query(resolver, &n, A, true).ok().and_then(|a| a.ips().first().copied()).map(|ip| (n, ip))).collect()
}

/// What one of the domain's own nameservers says. If the host points (CNAME)
/// into another zone, that zone's nameservers are asked for the rest.
fn own_view(resolver: &Server, host: &str, ns: &str, ip: IpAddr) -> View {
    let mut v = View::new(Source::Own, ns.to_owned());
    fill(&mut v, &Server::Udp(ip), host, false);
    if v.error.is_none() {
        // Not authoritative: the domain points here, but the server doesn't know it.
        if let Ok(a) = dns::query(&Server::Udp(ip), host, A, false) {
            if !a.authoritative {
                v.error = Some("ist gar nicht zuständig (antwortet nicht verbindlich)".into());
            }
        }
    }
    let mut depth = 0;
    while v.ips.is_empty() && v.error.is_none() && !v.nxdomain && depth < 4 {
        let Some(target) = v.chain.last().cloned() else { break };
        let Some(next_ns) = zone_of(resolver, &target).map(|z| nameservers(resolver, &z)).and_then(|n| n.first().map(|(_, ip)| *ip)) else { break };
        let mut rest = View::new(Source::Own, String::new());
        fill(&mut rest, &Server::Udp(next_ns), &target, false);
        v.chain.extend(rest.chain.into_iter().skip(1));
        v.ips = rest.ips;
        v.ttl = rest.ttl.or(v.ttl);
        v.error = rest.error;
        depth += 1;
    }
    v
}

fn mac_view(host: &str) -> View {
    let mut v = View::new(Source::Mac, "Dieser Mac".into());
    match (host, 443).to_socket_addrs() {
        Ok(addrs) => {
            let set: BTreeSet<IpAddr> = addrs.map(|a| a.ip()).collect();
            v.ips = set.into_iter().collect();
        }
        Err(_) => v.nxdomain = true,
    }
    v
}

fn rcode_text(rcode: u8) -> String {
    match rcode {
        2 => "Serverfehler (SERVFAIL)".into(),
        5 => "verweigert die Antwort".into(),
        n => format!("Fehler {n}"),
    }
}

fn io_text(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => "antwortet nicht".into(),
        _ => e.to_string(),
    }
}

/// Every address the domain has: from its own nameservers, else from anyone.
fn dns_addresses(d: &Dns, host: &str) -> Vec<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return vec![ip];
    }
    let pick = |own: bool| -> BTreeSet<IpAddr> { d.views.iter().filter(|v| (v.source == Source::Own) == own && v.answered()).flat_map(|v| v.ips.iter().copied()).collect() };
    let own = pick(true);
    let all = if own.is_empty() { pick(false) } else { own };
    all.into_iter().collect()
}

// ── The path ─────────────────────────────────────────────────────────────────

fn follow(start: &Url, guessed_scheme: bool) -> Vec<Hop> {
    let mut hops: Vec<Hop> = Vec::new();
    let mut url = start.clone();
    for _ in 0..MAX_HOPS {
        let hop = match web::get(&url, None) {
            Ok(reply) => {
                let redirected_by = reply.location.is_some().then(|| who_redirected(&reply));
                Hop { url: url.to_string(), reply: Some(reply), error: None, redirected_by }
            }
            Err(e) => Hop { url: url.to_string(), reply: None, error: Some(e), redirected_by: None },
        };
        let next = hop.reply.as_ref().filter(|r| (300..400).contains(&r.status)).and_then(|r| r.location.as_deref()).and_then(|l| url.join(l).ok());
        // Typed without http(s) and nothing answers on port 80: maybe only https works.
        let retry_https = hop.error.is_some() && hops.is_empty() && guessed_scheme && url.scheme() == "http";
        let looped = next.as_ref().is_some_and(|n| hops.iter().any(|h| h.url == n.as_str()) || n.as_str() == hop.url);
        hops.push(hop);
        if retry_https {
            let _ = url.set_scheme("https");
            continue;
        }
        match next {
            Some(n) if !looped => url = n,
            Some(n) => {
                hops.push(Hop { url: n.to_string(), reply: None, error: Some("schon da gewesen – eine Schleife".into()), redirected_by: None });
                break;
            }
            None => break,
        }
    }
    hops
}

fn who_redirected(r: &web::Reply) -> String {
    if let Some(by) = &r.redirect_by {
        return by.clone();
    }
    match r.server.as_deref() {
        _ if r.cloudflare => "Cloudflare".into(),
        Some(s) => format!("Webserver ({s})"),
        None => "Webserver".into(),
    }
}

// ── Every address ────────────────────────────────────────────────────────────

fn each_address(url: &Url, host: &str, ips: &[IpAddr]) -> Vec<Address> {
    let ipv6 = has_ipv6();
    std::thread::scope(|s| {
        let handles: Vec<_> = ips
            .iter()
            .map(|&ip| {
                s.spawn(move || {
                    let mut a = Address { host: host.to_owned(), ip, status: None, ms: None, server: None, error: None, untested: false };
                    if ip.is_ipv6() && !ipv6 {
                        a.untested = true;
                        return a;
                    }
                    match web::get(url, Some(ip)) {
                        Ok(r) => {
                            a.status = Some(r.status);
                            a.ms = Some(r.ms);
                            a.server = r.server;
                        }
                        Err(e) => a.error = Some(e),
                    }
                    a
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("no panics")).collect()
    })
}

/// Whether this Mac can reach the IPv6 internet at all (no packet is sent).
fn has_ipv6() -> bool {
    UdpSocket::bind("[::]:0").and_then(|s| s.connect("[2001:4860:4860::8888]:53")).is_ok()
}

// ── Findings ─────────────────────────────────────────────────────────────────

fn ip_list(ips: &[IpAddr]) -> String {
    if ips.is_empty() {
        "keine Adresse".into()
    } else {
        ips.iter().map(IpAddr::to_string).collect::<Vec<_>>().join(", ")
    }
}

/// `3 Std. 20 Min.`, `45 Min.`, `30 Sek.`
pub fn duration(secs: u32) -> String {
    match secs {
        0..=59 => format!("{secs} Sek."),
        60..=3599 => format!("{} Min.", secs.div_ceil(60)),
        _ if secs % 3600 < 60 => format!("{} Std.", secs / 3600),
        _ => format!("{} Std. {} Min.", secs / 3600, secs % 3600 / 60),
    }
}

fn findings(c: &Check) -> Vec<Finding> {
    let mut f = Vec::new();
    let mut add = |level, area, text: String| f.push(Finding { level, area, text });
    let d = &c.dns;
    let own: Vec<&View> = d.views.iter().filter(|v| v.source == Source::Own).collect();
    let own_ok: Vec<&View> = own.iter().copied().filter(|v| v.answered()).collect();
    let public: Vec<&View> = d.views.iter().filter(|v| v.source == Source::Public).collect();
    let is_ip = c.host.parse::<IpAddr>().is_ok();

    // DNS
    if !is_ip {
        let truth = own_ok.first().map(|v| (v.ips.clone(), v.nxdomain));
        if d.port53_blocked {
            add(Level::Odd, Area::Dns, "In diesem Netz ist DNS (Port 53) gesperrt – die eigenen Nameserver der Domain konnte ich nicht fragen, die öffentlichen nur über HTTPS.".into());
        }
        if d.zone.is_none() && public.iter().all(|v| v.nxdomain) {
            add(Level::Problem, Area::Dns, format!("Die Domain „{}“ gibt es im DNS nicht – abgelaufen, vertippt oder nie eingerichtet?", c.host));
        } else if !own.is_empty() && own_ok.is_empty() && !d.port53_blocked {
            add(Level::Problem, Area::Dns, format!("Die Nameserver der Domain antworten nicht: {}.", own.iter().map(|v| format!("{} {}", v.who, v.error.as_deref().unwrap_or(""))).collect::<Vec<_>>().join("; ")));
        } else if let Some((ips, nx)) = &truth {
            for v in own.iter().filter(|v| !v.answered()) {
                add(Level::Problem, Area::Dns, format!("Der Nameserver {} {} – die anderen springen ein, aber das gehört repariert.", v.who, v.error.as_deref().unwrap_or("hakt")));
            }
            let own_ips: BTreeSet<IpAddr> = own_ok.iter().flat_map(|v| v.ips.iter().copied()).collect();
            if own_ok.iter().any(|v| v.ips != *ips || v.nxdomain != *nx) {
                // Big providers hand out different addresses on purpose (load, location); that's
                // only a problem if one of them doesn't work as well — see „every address“ below.
                let all_fine = {
                    let checked: Vec<&Address> = c.addresses.iter().filter(|a| a.host == c.host && !a.untested).collect();
                    !checked.is_empty() && checked.iter().all(|a| a.error.is_none() && a.status == checked[0].status)
                };
                let text = format!("Die Nameserver der Domain sind sich uneinig: {}.", own_ok.iter().map(|v| format!("{} sagt {}", v.who, ip_list(&v.ips))).collect::<Vec<_>>().join(", "));
                if all_fine {
                    add(Level::Odd, Area::Dns, format!("{text} Alle diese Adressen antworten gleich – große Anbieter verteilen so absichtlich die Last. Bei einer normalen Firmenseite ist es eher ein vergessener alter Eintrag."));
                } else {
                    add(Level::Problem, Area::Dns, format!("{text} Je nachdem, wen man fragt, landet man woanders."));
                }
            } else if *nx {
                add(Level::Problem, Area::Dns, format!("„{}“ ist bei den eigenen Nameservern der Domain nicht eingetragen.", c.host));
            } else if ips.is_empty() {
                add(Level::Problem, Area::Dns, format!("Für „{}“ ist keine Adresse eingetragen.", c.host));
            } else {
                let behind: Vec<&&View> = public.iter().filter(|v| v.answered() && !v.ips.iter().all(|ip| own_ips.contains(ip))).collect();
                for v in &behind {
                    let wait = v.ttl.map(|t| format!(" – das ist spätestens in {} durch", duration(t))).unwrap_or_default();
                    add(Level::Odd, Area::Dns, format!("{} hat noch {} statt {}{wait}. Eine DNS-Änderung ist noch nicht überall angekommen.", v.who, ip_list(&v.ips), ip_list(ips)));
                }
                if behind.is_empty() {
                    add(Level::Ok, Area::Dns, format!("DNS ist überall gleich: {} {}.", c.host, ip_list(ips)));
                }
                if let Some(mac) = d.views.iter().find(|v| v.source == Source::Mac) {
                    if !mac.ips.is_empty() && !mac.ips.iter().all(|ip| own_ips.contains(ip)) && behind.is_empty() {
                        add(Level::Odd, Area::Dns, format!("Dieser Mac sieht {} statt {} – ein Eintrag in /etc/hosts oder ein alter Zwischenspeicher?", ip_list(&mac.ips), ip_list(ips)));
                    }
                }
            }
        }
    }

    // Every address, per name
    let hosts: BTreeSet<&str> = c.addresses.iter().map(|a| a.host.as_str()).collect();
    for host in hosts {
        let tested: Vec<&Address> = c.addresses.iter().filter(|a| a.host == host && !a.untested).collect();
        let answering: Vec<&Address> = tested.iter().copied().filter(|a| a.error.is_none()).collect();
        if answering.is_empty() {
            continue; // nothing answers at all: the path says so
        }
        for a in tested.iter().filter(|a| a.error.is_some()) {
            let v6 = if a.ip.is_ipv6() { " Ein alter IPv6-Eintrag (AAAA) zeigt oft noch auf den früheren Server – dann geht die Seite für manche Besucher und für andere nicht." } else { "" };
            add(Level::Problem, Area::Server, format!("{host} unter {} antwortet nicht ({}), die anderen Adressen schon.{v6}", a.ip, a.error.as_deref().unwrap_or("")));
        }
        let statuses: BTreeSet<u16> = answering.iter().filter_map(|a| a.status).collect();
        if statuses.len() > 1 {
            add(Level::Problem, Area::Server, format!("Die Server von {host} antworten unterschiedlich: {}. Einer davon ist nicht auf dem gleichen Stand.", answering.iter().map(|a| format!("{} → {}", a.ip, a.status.unwrap_or(0))).collect::<Vec<_>>().join(", ")));
        } else if tested.len() > 1 && answering.len() == tested.len() {
            add(Level::Ok, Area::Server, format!("Alle {} Adressen von {host} antworten gleich.", tested.len()));
        }
    }
    if c.addresses.iter().any(|a| a.untested) {
        add(Level::Ok, Area::Server, "IPv6 kann ich von diesem Netz aus nicht prüfen.".into());
    }

    // The path
    let last = c.path.last();
    let loop_found = last.and_then(|h| h.error.as_deref()).is_some_and(|e| e.contains("Schleife"));
    if loop_found {
        let wp = c.path.iter().any(|h| h.redirected_by.as_deref().is_some_and(|b| b.contains("WordPress") || b.contains("Yoast")));
        let area = if wp { Area::WordPress } else { Area::Server };
        let culprits: BTreeSet<&str> = c.path.iter().filter_map(|h| h.redirected_by.as_deref()).collect();
        add(Level::Problem, area, format!("Die Weiterleitungen drehen sich im Kreis (Umleitung durch {}). Oft streiten sich Cloudflare/Webserver und WordPress, ob mit oder ohne www bzw. https.", culprits.into_iter().collect::<Vec<_>>().join(" und ")));
    } else if let Some(err) = last.and_then(|h| h.error.as_deref()) {
        let area = if c.dns.views.iter().all(|v| v.ips.is_empty()) && !is_ip { Area::Dns } else { Area::Server };
        add(Level::Problem, area, format!("Der Webserver ist nicht zu erreichen: {err}."));
    } else if c.path.len() >= MAX_HOPS {
        add(Level::Problem, Area::Server, format!("Nach {MAX_HOPS} Weiterleitungen hab ich aufgegeben."));
    }
    if c.path.len() > 4 && !loop_found {
        add(Level::Odd, Area::Server, format!("{} Weiterleitungen bis zur Seite – das kostet jedes Mal Zeit. Besser direkt auf die Endadresse.", c.path.len() - 1));
    }
    // Typed without https, port 80 dead, https fine: the retry in `follow`.
    if let (Some(first), true) = (c.path.first(), c.path.iter().skip(1).any(|h| h.reply.is_some())) {
        if let Some(e) = &first.error {
            add(Level::Odd, Area::Server, format!("Auf {} antwortet nichts ({e}), auf https schon. Wer die Adresse ohne https eintippt, landet je nach Browser im Nichts.", first.url));
        }
    }
    let starts_http = c.path.first().is_some_and(|h| h.url.starts_with("http://") && h.reply.is_some());
    let ends_https = last.is_some_and(|h| h.url.starts_with("https://"));
    if let Some(p) = &c.page {
        if starts_http && !ends_https && p.status < 400 {
            add(Level::Odd, Area::Server, "Die Seite bleibt auf http – es gibt keine Weiterleitung auf https.".into());
        } else if starts_http && ends_https {
            add(Level::Ok, Area::Server, "http leitet auf https um.".into());
        }
        if c.path.iter().skip(1).any(|h| h.url.starts_with("http://")) && c.path.first().is_some_and(|h| h.url.starts_with("https://")) {
            add(Level::Odd, Area::Server, "Unterwegs geht's von https zurück auf http.".into());
        }
    }

    // The final page
    if let (Some(p), Some(r)) = (&c.page, last.and_then(|h| h.reply.as_ref())) {
        let wp = if p.wordpress { Area::WordPress } else { Area::Server };
        match p.status {
            _ if p.maintenance => add(Level::Problem, Area::WordPress, "WordPress steckt im Wartungsmodus – ein Update ist hängen geblieben. Die Datei .maintenance im WordPress-Ordner löschen hilft meistens.".into()),
            500 => add(Level::Problem, wp, if p.wordpress { "WordPress (bzw. PHP) hat einen Fehler (500). Ins Fehlerprotokoll beim Hoster gucken; oft ein Plugin oder Theme.".into() } else { "Der Server meldet einen internen Fehler (500).".into() }),
            502 | 504 => add(Level::Problem, Area::Server, format!("Der Server meldet {} – er selbst läuft, aber was dahinter (PHP, die eigentliche Seite) antwortet nicht.", p.status)),
            503 => add(Level::Problem, Area::Server, "Der Server meldet 503 (nicht verfügbar) – überlastet oder absichtlich abgeschaltet.".into()),
            404 => add(Level::Problem, wp, "Die Seite gibt's nicht (404).".into()),
            401 | 403 => add(Level::Problem, Area::Server, format!("Zugriff verweigert ({}) – Passwortschutz, Firewall oder ein Sicherheits-Plugin?", p.status)),
            s if s >= 400 => add(Level::Problem, wp, format!("Die Seite antwortet mit Fehler {s}.")),
            s if s < 300 && r.ms > SLOW_MS => add(Level::Odd, Area::Server, format!("Die Seite lädt, braucht aber {:.1} Sekunden für die erste Antwort – bei WordPress fehlt oft ein Cache.", r.ms as f64 / 1000.0).replace('.', ",")),
            s if s < 300 => add(Level::Ok, Area::Page, format!("Die Seite lädt ({s}) in {} ms{}.", r.ms, p.title.as_deref().map(|t| format!(": „{t}“")).unwrap_or_default())),
            _ => {}
        }
        if let Some(how) = &p.noindex {
            add(Level::Odd, Area::Page, format!("Suchmaschinen sind ausgesperrt (noindex im {how}) – nach einem Relaunch oft vergessen. In WordPress: Einstellungen → Lesen → „Suchmaschinen davon abhalten …“."));
        }
        if let Some(to) = &p.meta_refresh {
            add(Level::Odd, Area::Page, format!("Die Seite leitet per Meta-Refresh weiter (nach {to}) – das sieht der Server nicht, Suchmaschinen mögen's nicht."));
        }
    }

    f.sort_by_key(|x| std::cmp::Reverse(x.level));
    f
}

fn verdict(findings: &[Finding]) -> Verdict {
    match findings.first() {
        Some(f) if f.level == Level::Problem => match f.area {
            Area::Dns => Verdict::Dns,
            Area::WordPress => Verdict::WordPress,
            Area::Server | Area::Page => Verdict::Server,
        },
        Some(f) if f.level == Level::Odd => Verdict::Odd,
        _ => Verdict::Fine,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn understands_what_people_paste() {
        assert_eq!(parse("example.de").unwrap().as_str(), "http://example.de/");
        assert_eq!(parse("  https://www.example.de/kontakt?x=1 ").unwrap().host_str(), Some("www.example.de"));
        assert_eq!(parse("www.müller-bau.de").unwrap().host_str(), Some("www.xn--mller-bau-q9a.de"), "umlauts become punycode");
        assert!(parse("ftp://example.de").is_err());
        assert!(parse("hallo").is_err());
        assert!(parse("").is_err());
        assert!(parse("192.168.1.10").is_ok());
    }

    #[test]
    fn durations() {
        assert_eq!(duration(30), "30 Sek.");
        assert_eq!(duration(300), "5 Min.");
        assert_eq!(duration(3600), "1 Std.");
        assert_eq!(duration(12_000), "3 Std. 20 Min.");
    }

    fn view(source: Source, who: &str, ips: &[&str], ttl: Option<u32>) -> View {
        View { source, who: who.into(), chain: Vec::new(), ips: ips.iter().map(|i| i.parse().unwrap()).collect(), ttl, nxdomain: false, error: None }
    }

    fn reply(status: u16, location: Option<&str>, redirect_by: Option<&str>) -> web::Reply {
        web::Reply { status, location: location.map(Into::into), redirect_by: redirect_by.map(Into::into), ms: 120, ..Default::default() }
    }

    fn base() -> Check {
        Check {
            input: "example.de".into(),
            url: "http://example.de/".into(),
            host: "example.de".into(),
            at: "11.11.2026, 16:45".into(),
            dns: Dns {
                zone: Some("example.de".into()),
                nameservers: vec!["ns1.hoster.de".into(), "ns2.hoster.de".into()],
                views: vec![
                    view(Source::Own, "ns1.hoster.de", &["203.0.113.5", "2001:db8::5"], Some(3600)),
                    view(Source::Own, "ns2.hoster.de", &["203.0.113.5", "2001:db8::5"], Some(3600)),
                    view(Source::Public, "Google (8.8.8.8)", &["203.0.113.5", "2001:db8::5"], Some(1800)),
                    view(Source::Mac, "Dieser Mac", &["203.0.113.5", "2001:db8::5"], None),
                ],
                port53_blocked: false,
            },
            path: vec![
                Hop { url: "http://example.de/".into(), reply: Some(reply(301, Some("https://example.de/"), None)), error: None, redirected_by: Some("Webserver (nginx)".into()) },
                Hop { url: "https://example.de/".into(), reply: Some(reply(200, None, None)), error: None, redirected_by: None },
            ],
            addresses: vec![
                Address { host: "example.de".into(), ip: "203.0.113.5".parse().unwrap(), status: Some(200), ms: Some(100), server: None, error: None, untested: false },
                Address { host: "example.de".into(), ip: "2001:db8::5".parse().unwrap(), status: Some(200), ms: Some(110), server: None, error: None, untested: false },
            ],
            page: Some(Page { url: "https://example.de/".into(), status: 200, title: Some("Start".into()), wordpress: true, noindex: None, maintenance: false, meta_refresh: None }),
            findings: Vec::new(),
            verdict: Verdict::Fine,
        }
    }

    fn judge(c: &mut Check) -> Verdict {
        c.findings = findings(c);
        verdict(&c.findings)
    }

    #[test]
    fn a_healthy_site_is_fine() {
        let mut c = base();
        assert_eq!(judge(&mut c), Verdict::Fine, "{:#?}", c.findings);
        assert!(c.findings.iter().any(|f| f.text.contains("DNS ist überall gleich")));
        assert!(c.findings.iter().any(|f| f.text.contains("http leitet auf https um")));
    }

    #[test]
    fn a_change_on_its_way_is_odd_not_a_problem() {
        let mut c = base();
        c.dns.views[2] = view(Source::Public, "Google (8.8.8.8)", &["198.51.100.9"], Some(5400));
        assert_eq!(judge(&mut c), Verdict::Odd);
        assert!(c.findings[0].text.contains("Google (8.8.8.8) hat noch 198.51.100.9") && c.findings[0].text.contains("spätestens in 1 Std. 30 Min."), "{}", c.findings[0].text);
    }

    #[test]
    fn a_stale_ipv6_record_is_the_servers_problem() {
        let mut c = base();
        c.addresses[1].status = None;
        c.addresses[1].error = Some("keine Antwort (Zeitüberschreitung)".into());
        assert_eq!(judge(&mut c), Verdict::Server);
        assert!(c.findings[0].text.contains("2001:db8::5") && c.findings[0].text.contains("AAAA"), "{}", c.findings[0].text);
    }

    #[test]
    fn nameservers_that_disagree_are_a_dns_problem() {
        let mut c = base();
        c.dns.views[1] = view(Source::Own, "ns2.hoster.de", &["198.51.100.9"], Some(3600));
        // The old server still listed at one nameserver serves an old copy.
        c.addresses.push(Address { host: "example.de".into(), ip: "198.51.100.9".parse().unwrap(), status: Some(404), ms: Some(90), server: None, error: None, untested: false });
        assert_eq!(judge(&mut c), Verdict::Dns);
        assert!(c.findings[0].text.contains("uneinig"));
    }

    #[test]
    fn wordpress_gets_the_blame_for_its_own_trouble() {
        let mut c = base();
        c.path[1].reply = Some(reply(500, None, None));
        c.page.as_mut().unwrap().status = 500;
        assert_eq!(judge(&mut c), Verdict::WordPress);

        let mut c = base();
        c.path.push(Hop { url: "https://www.example.de/".into(), reply: Some(reply(301, Some("https://example.de/"), Some("WordPress"))), error: None, redirected_by: Some("WordPress".into()) });
        c.path.push(Hop { url: "https://example.de/".into(), reply: None, error: Some("schon da gewesen – eine Schleife".into()), redirected_by: None });
        assert_eq!(judge(&mut c), Verdict::WordPress);
        assert!(c.findings[0].text.contains("im Kreis") && c.findings[0].text.contains("WordPress"), "{}", c.findings[0].text);
    }

    #[test]
    fn load_balancing_is_odd_but_a_dead_old_address_is_a_problem() {
        let mut c = base();
        c.dns.views[1] = view(Source::Own, "ns2.hoster.de", &["2001:db8::5"], Some(3600));
        assert_eq!(judge(&mut c), Verdict::Odd, "{:#?}", c.findings);
        assert!(c.findings[0].text.contains("absichtlich"));
        c.addresses[1].status = None;
        c.addresses[1].error = Some("keine Antwort (Zeitüberschreitung)".into());
        assert_eq!(judge(&mut c), Verdict::Dns);
    }

    #[test]
    fn slow_pages_are_mentioned() {
        let mut c = base();
        c.path[1].reply.as_mut().unwrap().ms = 5275;
        assert_eq!(judge(&mut c), Verdict::Odd);
        assert!(c.findings[0].text.contains("5,3 Sekunden"), "{}", c.findings[0].text);
    }

    #[test]
    fn noindex_after_a_relaunch_is_odd() {
        let mut c = base();
        c.page.as_mut().unwrap().noindex = Some("Meta-Tag".into());
        assert_eq!(judge(&mut c), Verdict::Odd);
        assert!(c.findings[0].text.contains("noindex"));
    }

    #[test]
    fn an_unknown_domain_is_a_dns_problem() {
        let mut c = base();
        c.dns.zone = None;
        c.dns.views = vec![View { nxdomain: true, ..view(Source::Public, "Google (8.8.8.8)", &[], None) }, View { nxdomain: true, ..view(Source::Mac, "Dieser Mac", &[], None) }];
        c.path = vec![Hop { url: "http://example.de/".into(), reply: None, error: Some("Name nicht gefunden".into()), redirected_by: None }];
        c.addresses.clear();
        c.page = None;
        assert_eq!(judge(&mut c), Verdict::Dns);
        assert!(c.findings[0].text.contains("gibt es im DNS nicht"));
    }
}
