//! A small DNS client: just enough to ask any server (a public resolver or the
//! domain's own nameserver) for A, AAAA, CNAME, NS and SOA records and read the
//! answer. UDP, TCP when the answer doesn't fit, or DNS over HTTPS when port 53
//! is blocked on the network.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

pub const A: u16 = 1;
pub const NS: u16 = 2;
pub const CNAME: u16 = 5;
pub const SOA: u16 = 6;
pub const AAAA: u16 = 28;

const TIMEOUT: Duration = Duration::from_secs(2);
/// Answers bigger than this come over TCP (EDNS0 buffer size, the DNS-flag-day value).
const UDP_SIZE: u16 = 1232;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Ip(IpAddr),
    /// NS, CNAME, and the primary nameserver of an SOA.
    Name(String),
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub kind: u16,
    pub ttl: u32,
    pub data: Data,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    /// 0 ok, 2 server failure, 3 the name doesn't exist, 5 refused.
    pub rcode: u8,
    /// From the domain's own nameserver, not a cache.
    pub authoritative: bool,
    pub answers: Vec<Record>,
    pub authority: Vec<Record>,
    pub additional: Vec<Record>,
}

pub const NXDOMAIN: u8 = 3;

/// How to reach a DNS server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Server {
    Udp(IpAddr),
    /// DNS over HTTPS, e.g. `https://cloudflare-dns.com/dns-query`.
    Https(&'static str),
}

/// Asks `server` for `name`/`kind`. `recursive`: let a resolver look it up
/// (public resolvers); a domain's own nameserver is asked without.
pub fn query(server: &Server, name: &str, kind: u16, recursive: bool) -> std::io::Result<Answer> {
    let id = fastrand::u16(..);
    let packet = encode(id, name, kind, recursive);
    let reply = match server {
        Server::Udp(ip) => {
            let addr = SocketAddr::new(*ip, 53);
            let reply = udp(addr, &packet)?;
            // TC bit: truncated, ask again over TCP.
            if reply.len() > 2 && reply[2] & 0x02 != 0 {
                tcp(addr, &packet)?
            } else {
                reply
            }
        }
        Server::Https(url) => crate::web::doh(url, &packet)?,
    };
    let answer = decode(&reply).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "kaputte DNS-Antwort"))?;
    if u16::from_be_bytes([reply[0], reply[1]]) != id && matches!(server, Server::Udp(_)) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "falsche DNS-Antwort"));
    }
    Ok(answer)
}

fn udp(addr: SocketAddr, packet: &[u8]) -> std::io::Result<Vec<u8>> {
    let socket = UdpSocket::bind(if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" })?;
    socket.set_read_timeout(Some(TIMEOUT))?;
    socket.connect(addr)?;
    let mut buf = vec![0; 65_535];
    // One retry: a single lost packet shouldn't count as „antwortet nicht“.
    for attempt in 0..2 {
        socket.send(packet)?;
        match socket.recv(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                return Ok(buf);
            }
            Err(e) if attempt == 1 => return Err(e),
            Err(_) => {}
        }
    }
    unreachable!()
}

fn tcp(addr: SocketAddr, packet: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut s = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    s.set_read_timeout(Some(TIMEOUT))?;
    s.write_all(&(packet.len() as u16).to_be_bytes())?;
    s.write_all(packet)?;
    let mut len = [0; 2];
    s.read_exact(&mut len)?;
    let mut buf = vec![0; u16::from_be_bytes(len) as usize];
    s.read_exact(&mut buf)?;
    Ok(buf)
}

/// A query: header, one question, and an EDNS0 record for bigger UDP answers.
pub fn encode(id: u16, name: &str, kind: u16, recursive: bool) -> Vec<u8> {
    let mut p = Vec::with_capacity(64);
    p.extend(id.to_be_bytes());
    p.extend([if recursive { 0x01 } else { 0x00 }, 0x00]); // RD
    p.extend([0, 1, 0, 0, 0, 0, 0, 1]); // 1 question, 1 additional (OPT)
    for label in name.trim_end_matches('.').split('.').filter(|l| !l.is_empty()) {
        p.push(label.len().min(63) as u8);
        p.extend(&label.as_bytes()[..label.len().min(63)]);
    }
    p.push(0);
    p.extend(kind.to_be_bytes());
    p.extend(1u16.to_be_bytes()); // IN
    p.extend([0, 0, 41]); // root, OPT
    p.extend(UDP_SIZE.to_be_bytes());
    p.extend([0, 0, 0, 0, 0, 0]); // extended rcode, version, flags, no options
    p
}

pub fn decode(p: &[u8]) -> Option<Answer> {
    if p.len() < 12 {
        return None;
    }
    let count = |i: usize| u16::from_be_bytes([p[i], p[i + 1]]) as usize;
    let mut a = Answer { rcode: p[3] & 0x0f, authoritative: p[2] & 0x04 != 0, ..Default::default() };
    let mut pos = 12;
    for _ in 0..count(4) {
        let (_, next) = read_name(p, pos)?;
        pos = next + 4;
    }
    for (n, list) in [(count(6), &mut a.answers), (count(8), &mut a.authority), (count(10), &mut a.additional)] {
        for _ in 0..n {
            let (name, next) = read_name(p, pos)?;
            let head = p.get(next..next + 10)?;
            let kind = u16::from_be_bytes([head[0], head[1]]);
            let ttl = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
            let len = u16::from_be_bytes([head[8], head[9]]) as usize;
            let start = next + 10;
            let rdata = p.get(start..start + len)?;
            let data = match (kind, len) {
                (A, 4) => Data::Ip(IpAddr::V4(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3]))),
                (AAAA, 16) => Data::Ip(IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(rdata).ok()?))),
                (NS | CNAME | SOA, _) => Data::Name(read_name(p, start)?.0),
                _ => Data::Other,
            };
            if kind != 41 {
                list.push(Record { name, kind, ttl, data });
            }
            pos = start + len;
        }
    }
    Some(a)
}

/// A (possibly compressed) name at `pos`; returns it lowercased without the
/// final dot, and the position after it.
fn read_name(p: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut end = None;
    for _ in 0..128 {
        let len = *p.get(pos)? as usize;
        match len {
            0 => {
                return Some((labels.join("."), end.unwrap_or(pos + 1)));
            }
            l if l & 0xc0 == 0xc0 => {
                let target = (l & 0x3f) << 8 | *p.get(pos + 1)? as usize;
                end.get_or_insert(pos + 2);
                if target >= pos {
                    return None; // pointers only point back; anything else loops
                }
                pos = target;
            }
            l => {
                labels.push(String::from_utf8_lossy(p.get(pos + 1..pos + 1 + l)?).to_lowercase());
                pos += 1 + l;
            }
        }
    }
    None
}

impl Answer {
    pub fn ips(&self) -> Vec<IpAddr> {
        let mut v: Vec<IpAddr> = self.answers.iter().filter_map(|r| if let Data::Ip(ip) = r.data { Some(ip) } else { None }).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn names(&self, kind: u16) -> Vec<String> {
        self.answers.iter().chain(&self.authority).filter(|r| r.kind == kind).filter_map(|r| if let Data::Name(n) = &r.data { Some(n.clone()) } else { None }).collect()
    }

    /// `name`'s CNAME target in this answer, if any.
    pub fn cname_of(&self, name: &str) -> Option<String> {
        self.answers.iter().find(|r| r.kind == CNAME && r.name == name).and_then(|r| if let Data::Name(n) = &r.data { Some(n.clone()) } else { None })
    }

    /// Shortest TTL of the address records: how long a resolver keeps them at most.
    pub fn address_ttl(&self) -> Option<u32> {
        self.answers.iter().filter(|r| matches!(r.kind, A | AAAA)).map(|r| r.ttl).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reply for `www.example.de` A: a CNAME to `example.de` (compressed) and its address.
    fn reply() -> Vec<u8> {
        let mut p = vec![0x12, 0x34, 0x85, 0x80, 0, 1, 0, 2, 0, 0, 0, 0];
        p.extend(b"\x03www\x07Example\x02de\x00\x00\x01\x00\x01");
        // www.example.de CNAME example.de (pointer to offset 16 = "example.de")
        p.extend([0xc0, 12, 0, 5, 0, 1, 0, 0, 0x0e, 0x10, 0, 2, 0xc0, 16]);
        // example.de A 93.184.216.34, TTL 300
        p.extend([0xc0, 16, 0, 1, 0, 1, 0, 0, 1, 0x2c, 0, 4, 93, 184, 216, 34]);
        p
    }

    #[test]
    fn reads_answers_with_compressed_names() {
        let a = decode(&reply()).unwrap();
        assert!(a.authoritative);
        assert_eq!(a.rcode, 0);
        assert_eq!(a.cname_of("www.example.de").as_deref(), Some("example.de"));
        assert_eq!(a.ips(), ["93.184.216.34".parse::<IpAddr>().unwrap()]);
        assert_eq!(a.address_ttl(), Some(300));
    }

    #[test]
    fn writes_a_query() {
        let q = encode(0xabcd, "www.example.de.", AAAA, false);
        assert_eq!(&q[..4], &[0xab, 0xcd, 0, 0], "no recursion for the domain's own nameservers");
        assert_eq!(&q[12..28], b"\x03www\x07example\x02de\x00");
        assert_eq!(&q[28..32], &[0, 28, 0, 1]);
        assert!(encode(1, "x.de", A, true)[2] & 1 == 1);
    }

    #[test]
    fn broken_replies_are_refused() {
        let mut r = reply();
        r.truncate(40);
        assert!(decode(&r).is_none());
        // A pointer pointing at itself.
        let mut looped = vec![0, 0, 0x81, 0x80, 0, 1, 0, 0, 0, 0, 0, 0];
        looped.extend([0xc0, 12, 0, 1, 0, 1]);
        assert!(decode(&looped).is_none());
    }
}
