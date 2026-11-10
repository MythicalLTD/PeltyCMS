//! Game server status queries: Minecraft Java (Server List Ping), Minecraft
//! Bedrock (RakNet unconnected ping), Source/A2S (`A2S_INFO`, used by Rust,
//! CS2, ARK, Garry's Mod…) and FiveM/RedM (`/info.json` + `/players.json`).
//! Every query has a short timeout and returns a [`Status`] or an error text.
use serde::Serialize;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Status {
    pub players: i32,
    pub max_players: i32,
    pub version: String,
    pub motd: String,
    /// Up to 12 player names when the server shares them.
    pub sample: Vec<String>,
    pub latency_ms: i32,
}

/// Query `host:port` with `protocol` (`minecraft`, `bedrock`, `a2s`, `fivem`).
pub async fn query(protocol: &str, host: &str, port: u16) -> Result<Status, String> {
    if host.trim().is_empty() {
        return Err("no host".into());
    }
    let started = Instant::now();
    let fut = async {
        match protocol {
            "minecraft" => minecraft(host, port).await,
            "bedrock" => bedrock(host, port).await,
            "a2s" => a2s(host, port).await,
            "fivem" => fivem(host, port).await,
            other => Err(format!("no status query for {other}")),
        }
    };
    let mut st = tokio::time::timeout(TIMEOUT * 2, fut)
        .await
        .map_err(|_| "timed out".to_string())??;
    st.latency_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
    st.sample.truncate(12);
    Ok(st)
}

// ------------------------------------------------------------ Minecraft Java

pub(crate) fn varint(v: i32, out: &mut Vec<u8>) {
    let mut u = v as u32;
    while u & !0x7f != 0 {
        out.push(((u & 0x7f) | 0x80) as u8);
        u >>= 7;
    }
    out.push(u as u8);
}

async fn read_varint(s: &mut TcpStream) -> Result<i32, String> {
    let mut n: u32 = 0;
    for i in 0..5 {
        let b = s.read_u8().await.map_err(|e| e.to_string())?;
        n |= u32::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(n as i32);
        }
    }
    Err("bad varint".into())
}

fn packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = vec![];
    varint(id, &mut inner);
    inner.extend_from_slice(body);
    let mut out = vec![];
    varint(inner.len() as i32, &mut out);
    out.extend(inner);
    out
}

async fn minecraft(host: &str, port: u16) -> Result<Status, String> {
    let mut s = tokio::time::timeout(TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| "connect timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let mut hs = vec![];
    varint(-1, &mut hs); // protocol version: "any"
    varint(host.len() as i32, &mut hs);
    hs.extend_from_slice(host.as_bytes());
    hs.extend_from_slice(&port.to_be_bytes());
    varint(1, &mut hs); // next state: status
    let mut req = packet(0, &hs);
    req.extend(packet(0, &[]));
    s.write_all(&req).await.map_err(|e| e.to_string())?;
    let read = async {
        let _len = read_varint(&mut s).await?;
        let _id = read_varint(&mut s).await?;
        let n = read_varint(&mut s).await?;
        if !(0..=1 << 20).contains(&n) {
            return Err("bad status length".to_string());
        }
        let mut buf = vec![0; n as usize];
        s.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
        Ok(buf)
    };
    let buf = tokio::time::timeout(TIMEOUT, read)
        .await
        .map_err(|_| "read timed out".to_string())??;
    parse_slp(&String::from_utf8_lossy(&buf))
}

/// Flatten a chat component (string, `{text, extra}` or array) to text.
fn chat_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().map(chat_text).collect(),
        serde_json::Value::Object(o) => {
            let mut t = o
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(e) = o.get("extra") {
                t.push_str(&chat_text(e));
            }
            t
        }
        _ => String::new(),
    }
}

/// Remove `§x` formatting codes.
fn strip_codes(s: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in s.chars() {
        if skip {
            skip = false;
        } else if c == '§' {
            skip = true;
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

pub(crate) fn parse_slp(json: &str) -> Result<Status, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let p = &v["players"];
    Ok(Status {
        players: p["online"].as_i64().unwrap_or(0) as i32,
        max_players: p["max"].as_i64().unwrap_or(0) as i32,
        version: strip_codes(v["version"]["name"].as_str().unwrap_or("")),
        motd: strip_codes(&chat_text(&v["description"])),
        sample: p["sample"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        latency_ms: 0,
    })
}

// ------------------------------------------------------------ Bedrock

const RAKNET_MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];

/// UDP socket connected to the target, bound to the target's address family
/// (an IPv4 socket cannot reach an IPv6-only host).
async fn udp(host: &str, port: u16) -> Result<UdpSocket, String> {
    let addr = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    let local = if addr.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let sock = UdpSocket::bind(local).await.map_err(|e| e.to_string())?;
    sock.connect(addr).await.map_err(|e| e.to_string())?;
    Ok(sock)
}

async fn recv(sock: &UdpSocket) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(TIMEOUT, sock.recv(&mut buf))
        .await
        .map_err(|_| "no answer".to_string())?
        .map_err(|e| e.to_string())?;
    buf.truncate(n);
    Ok(buf)
}

async fn bedrock(host: &str, port: u16) -> Result<Status, String> {
    let sock = udp(host, port).await?;
    let mut req = vec![0x01];
    req.extend_from_slice(&0u64.to_be_bytes());
    req.extend_from_slice(&RAKNET_MAGIC);
    req.extend_from_slice(&0u64.to_be_bytes());
    sock.send(&req).await.map_err(|e| e.to_string())?;
    let buf = recv(&sock).await?;
    // 0x1c, time(8), guid(8), magic(16), len(2), id string
    if buf.len() < 35 || buf[0] != 0x1c {
        return Err("bad bedrock pong".into());
    }
    parse_bedrock(&String::from_utf8_lossy(&buf[35..]))
}

pub(crate) fn parse_bedrock(id: &str) -> Result<Status, String> {
    let f: Vec<&str> = id.split(';').collect();
    if f.len() < 6 || !matches!(f[0], "MCPE" | "MCEE") {
        return Err("bad bedrock pong".into());
    }
    Ok(Status {
        motd: strip_codes(f[1]),
        version: f[3].to_string(),
        players: f[4].parse().unwrap_or(0),
        max_players: f[5].parse().unwrap_or(0),
        ..Default::default()
    })
}

// ------------------------------------------------------------ A2S

const A2S_INFO: &[u8] = b"\xff\xff\xff\xffTSource Engine Query\0";
/// Header of a reply split over several datagrams (`-2` little endian).
const A2S_SPLIT: [u8; 4] = [0xfe, 0xff, 0xff, 0xff];
/// Cap on the parts of one split reply (Source sends at most a few).
const A2S_MAX_PARTS: u8 = 32;

async fn a2s(host: &str, port: u16) -> Result<Status, String> {
    let sock = udp(host, port).await?;
    sock.send(A2S_INFO).await.map_err(|e| e.to_string())?;
    let mut buf = a2s_reply(&sock).await?;
    // Challenge (0x41): resend with the 4 challenge bytes appended.
    if buf.len() >= 9 && buf[4] == 0x41 {
        let mut req = A2S_INFO.to_vec();
        req.extend_from_slice(&buf[5..9]);
        sock.send(&req).await.map_err(|e| e.to_string())?;
        buf = a2s_reply(&sock).await?;
    }
    parse_a2s(&buf)
}

/// One whole A2S reply: a single datagram, or every part of a split one
/// joined in order.
async fn a2s_reply(sock: &UdpSocket) -> Result<Vec<u8>, String> {
    let mut parts = A2sParts::default();
    for _ in 0..=usize::from(A2S_MAX_PARTS) * 2 {
        let buf = recv(sock).await?;
        if buf.starts_with(&A2S_SPLIT) {
            if let Some(whole) = parts.push(a2s_part(&buf)?)? {
                return Ok(whole);
            }
        } else {
            return Ok(buf);
        }
    }
    Err("incomplete A2S reply".into())
}

/// One datagram of a split reply (Source format: id, total, number, size).
#[derive(Debug)]
pub(crate) struct A2sPart {
    id: u32,
    total: u8,
    number: u8,
    payload: Vec<u8>,
}

pub(crate) fn a2s_part(buf: &[u8]) -> Result<A2sPart, String> {
    if buf.len() < 12 || buf[..4] != A2S_SPLIT {
        return Err("bad A2S split packet".into());
    }
    let id = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    // The high bit marks a bzip2-compressed reply (old engines only).
    if id & 0x8000_0000 != 0 {
        return Err("compressed A2S reply not supported".into());
    }
    let (total, number) = (buf[8], buf[9]);
    if total == 0 || total > A2S_MAX_PARTS || number >= total {
        return Err("bad A2S split packet".into());
    }
    // buf[10..12] is the part size; the datagram length is authoritative.
    Ok(A2sPart {
        id,
        total,
        number,
        payload: buf[12..].to_vec(),
    })
}

/// Collects the parts of one split reply; parts of another reply id (a late
/// answer to an earlier request) are ignored.
#[derive(Default)]
pub(crate) struct A2sParts {
    id: Option<u32>,
    parts: Vec<Option<Vec<u8>>>,
}

impl A2sParts {
    pub(crate) fn push(&mut self, p: A2sPart) -> Result<Option<Vec<u8>>, String> {
        match self.id {
            None => {
                self.id = Some(p.id);
                self.parts = vec![None; usize::from(p.total)];
            }
            Some(id) if id != p.id => return Ok(None),
            Some(_) if self.parts.len() != usize::from(p.total) => {
                return Err("bad A2S split packet".into())
            }
            Some(_) => {}
        }
        self.parts[usize::from(p.number)] = Some(p.payload);
        if self.parts.iter().all(Option::is_some) {
            Ok(Some(
                self.parts.iter().flatten().flatten().copied().collect(),
            ))
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn parse_a2s(buf: &[u8]) -> Result<Status, String> {
    if buf.len() < 6 || buf[..4] != [0xff; 4] || buf[4] != 0x49 {
        return Err("bad A2S_INFO reply".into());
    }
    let mut i = 6; // header + protocol byte
    let mut cstr = || -> Result<String, String> {
        let end = buf[i..].iter().position(|&b| b == 0).ok_or("truncated")?;
        let s = String::from_utf8_lossy(&buf[i..i + end]).to_string();
        i += end + 1;
        Ok(s)
    };
    let name = cstr()?;
    let _map = cstr()?;
    let _folder = cstr()?;
    let game = cstr()?;
    // app id (2), players (1), max (1)
    if buf.len() < i + 4 {
        return Err("truncated".into());
    }
    let players = i32::from(buf[i + 2]);
    let max = i32::from(buf[i + 3]);
    // version string follows: bots, type, os, visibility, vac (5 bytes)
    let j = i + 9;
    let version = buf
        .get(j..)
        .map(|rest| {
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            String::from_utf8_lossy(&rest[..end]).to_string()
        })
        .unwrap_or_default();
    Ok(Status {
        players,
        max_players: max,
        version: if version.is_empty() { game } else { version },
        motd: name,
        ..Default::default()
    })
}

// ------------------------------------------------------------ FiveM

async fn fivem(host: &str, port: u16) -> Result<Status, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let base = format!("http://{host}:{port}");
    let info: serde_json::Value = client
        .get(format!("{base}/info.json"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let players: serde_json::Value = client
        .get(format!("{base}/players.json"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse_fivem(&info, &players))
}

pub(crate) fn parse_fivem(info: &serde_json::Value, players: &serde_json::Value) -> Status {
    let vars = &info["vars"];
    let max = vars["sv_maxClients"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| vars["sv_maxClients"].as_i64().map(|n| n as i32))
        .unwrap_or(0);
    let list = players.as_array().cloned().unwrap_or_default();
    Status {
        players: list.len() as i32,
        max_players: max,
        version: info["server"].as_str().unwrap_or("").to_string(),
        motd: strip_codes(&strip_caret(vars["sv_projectName"].as_str().unwrap_or(""))),
        sample: list
            .iter()
            .filter_map(|p| p["name"].as_str().map(str::to_string))
            .collect(),
        latency_ms: 0,
    }
}

/// FiveM colour codes are `^0`–`^9`.
fn strip_caret(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '^' && it.peek().is_some_and(|d| d.is_ascii_digit()) {
            it.next();
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn varints() {
        let enc = |n| {
            let mut v = vec![];
            varint(n, &mut v);
            v
        };
        assert_eq!(enc(0), [0]);
        assert_eq!(enc(300), [0xac, 0x02]);
        assert_eq!(enc(-1), [0xff, 0xff, 0xff, 0xff, 0x0f]);
    }

    #[test]
    fn slp() {
        let s = parse_slp(r#"{"version":{"name":"Paper 1.21.1","protocol":767},"players":{"max":100,"online":2,"sample":[{"name":"Steve","id":"x"}]},"description":{"text":"§aHello ","extra":[{"text":"world"}]}}"#).unwrap();
        assert_eq!((s.players, s.max_players), (2, 100));
        assert_eq!(s.version, "Paper 1.21.1");
        assert_eq!(s.motd, "Hello world");
        assert_eq!(s.sample, ["Steve"]);
        assert!(parse_slp("not json").is_err());
    }

    #[test]
    fn bedrock_pong() {
        let s = parse_bedrock("MCPE;§bMy Server;712;1.21.2;5;50;123;World;Survival;1;19132;19133;")
            .unwrap();
        assert_eq!((s.players, s.max_players), (5, 50));
        assert_eq!(s.motd, "My Server");
        assert_eq!(s.version, "1.21.2");
        assert!(parse_bedrock("nope").is_err());
    }

    #[test]
    fn a2s_info() {
        let mut b = vec![0xff, 0xff, 0xff, 0xff, 0x49, 0x11];
        for s in ["Rusty Island", "Procedural Map", "rust", "Rust"] {
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        }
        b.extend_from_slice(&[0x00, 0x00, 7, 200, 0, b'd', b'l', 0, 1]);
        b.extend_from_slice(b"2593\0");
        let s = parse_a2s(&b).unwrap();
        assert_eq!((s.players, s.max_players), (7, 200));
        assert_eq!(s.motd, "Rusty Island");
        assert_eq!(s.version, "2593");
        assert!(parse_a2s(&[0xff; 4]).is_err());
    }

    fn split(id: u32, total: u8, number: u8, payload: &[u8]) -> Vec<u8> {
        let mut b = A2S_SPLIT.to_vec();
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&[total, number]);
        b.extend_from_slice(&1248u16.to_le_bytes());
        b.extend_from_slice(payload);
        b
    }

    #[test]
    fn a2s_split_reply() {
        let mut whole = vec![0xff, 0xff, 0xff, 0xff, 0x49, 0x11];
        for s in ["Big Server", "map", "rust", "Rust"] {
            whole.extend_from_slice(s.as_bytes());
            whole.push(0);
        }
        whole.extend_from_slice(&[0x00, 0x00, 3, 100, 0, b'd', b'l', 0, 1]);
        whole.extend_from_slice(b"2600\0");
        let (a, rest) = whole.split_at(10);
        let (b, c) = rest.split_at(10);
        let mut parts = A2sParts::default();
        // Out of order, with a stale part of another reply in between.
        assert!(parts
            .push(a2s_part(&split(7, 3, 2, c)).unwrap())
            .unwrap()
            .is_none());
        assert!(parts
            .push(a2s_part(&split(9, 3, 1, b"junk")).unwrap())
            .unwrap()
            .is_none());
        assert!(parts
            .push(a2s_part(&split(7, 3, 0, a)).unwrap())
            .unwrap()
            .is_none());
        let joined = parts
            .push(a2s_part(&split(7, 3, 1, b)).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(joined, whole);
        let s = parse_a2s(&joined).unwrap();
        assert_eq!((s.players, s.max_players), (3, 100));
        assert_eq!(s.version, "2600");

        let mut p = A2sParts::default();
        p.push(a2s_part(&split(1, 2, 0, b"x")).unwrap()).unwrap();
        assert!(p.push(a2s_part(&split(1, 3, 2, b"y")).unwrap()).is_err());
        assert!(a2s_part(&split(0x8000_0001, 2, 0, b"x")).is_err());
        assert!(a2s_part(&split(1, 2, 2, b"x")).is_err());
        assert!(a2s_part(&split(1, 0, 0, b"x")).is_err());
        assert!(a2s_part(&split(1, 33, 0, b"x")).is_err());
        assert!(a2s_part(&[0xfe, 0xff, 0xff, 0xff, 0]).is_err());
    }

    /// End to end over loopback UDP: challenge, then a reply in two parts.
    #[tokio::test]
    async fn a2s_over_udp_with_challenge_and_split() {
        let srv = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = srv.local_addr().unwrap().port();
        let mut whole = vec![0xff, 0xff, 0xff, 0xff, 0x49, 0x11];
        for s in ["Split Srv", "m", "cs", "CS"] {
            whole.extend_from_slice(s.as_bytes());
            whole.push(0);
        }
        whole.extend_from_slice(&[0x00, 0x00, 5, 32, 0, b'd', b'l', 0, 1]);
        whole.extend_from_slice(b"1.0\0");
        let task = tokio::spawn(async move {
            let mut b = [0u8; 1400];
            let (n, peer) = srv.recv_from(&mut b).await.unwrap();
            assert_eq!(&b[..n], A2S_INFO);
            srv.send_to(&[0xff, 0xff, 0xff, 0xff, 0x41, 1, 2, 3, 4], peer)
                .await
                .unwrap();
            let (n, peer) = srv.recv_from(&mut b).await.unwrap();
            assert_eq!(&b[n - 4..n], &[1, 2, 3, 4]);
            let (x, y) = whole.split_at(15);
            srv.send_to(&split(42, 2, 1, y), peer).await.unwrap();
            srv.send_to(&split(42, 2, 0, x), peer).await.unwrap();
        });
        let s = a2s("127.0.0.1", port).await.unwrap();
        task.await.unwrap();
        assert_eq!(s.motd, "Split Srv");
        assert_eq!((s.players, s.max_players), (5, 32));
    }

    /// IPv6-only targets get an IPv6 socket.
    #[tokio::test]
    async fn a2s_over_ipv6() {
        let Ok(srv) = UdpSocket::bind("[::1]:0").await else {
            return; // no IPv6 loopback on this machine
        };
        let port = srv.local_addr().unwrap().port();
        let mut whole = vec![0xff, 0xff, 0xff, 0xff, 0x49, 0x11];
        for s in ["Six", "m", "rust", "Rust"] {
            whole.extend_from_slice(s.as_bytes());
            whole.push(0);
        }
        whole.extend_from_slice(&[0x00, 0x00, 1, 10, 0, b'd', b'l', 0, 1]);
        let task = tokio::spawn(async move {
            let mut b = [0u8; 1400];
            let (_, peer) = srv.recv_from(&mut b).await.unwrap();
            srv.send_to(&whole, peer).await.unwrap();
        });
        let s = a2s("::1", port).await.unwrap();
        task.await.unwrap();
        assert_eq!(s.motd, "Six");
    }

    #[test]
    fn fivem_info() {
        let s = parse_fivem(
            &json!({"server": "FXServer-master v1.0.0.7290", "vars": {"sv_maxClients": "64", "sv_projectName": "^1Los ^7Santos RP"}}),
            &json!([{"name": "Ana"}, {"name": "Bo"}]),
        );
        assert_eq!((s.players, s.max_players), (2, 64));
        assert_eq!(s.motd, "Los Santos RP");
        assert_eq!(s.sample, ["Ana", "Bo"]);
    }
}
