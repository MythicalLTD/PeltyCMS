//! Minimal Source RCON client (Minecraft Java, Rust, Garry's Mod, ARK, CS2,
//! and every Source-engine server). One connection per batch: authenticate,
//! run commands, close.
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const AUTH: i32 = 3;
const EXEC: i32 = 2;
const TIMEOUT: Duration = Duration::from_secs(5);
/// Largest packet the protocol allows.
const MAX: i32 = 4096 + 10;

pub struct Rcon {
    stream: TcpStream,
    next: i32,
}

fn packet(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 14);
    b.extend_from_slice(&((body.len() + 10) as i32).to_le_bytes());
    b.extend_from_slice(&id.to_le_bytes());
    b.extend_from_slice(&kind.to_le_bytes());
    b.extend_from_slice(body.as_bytes());
    b.extend_from_slice(&[0, 0]);
    b
}

async fn read(stream: &mut TcpStream) -> Result<(i32, i32, String), String> {
    let len = tokio::time::timeout(TIMEOUT, stream.read_i32_le())
        .await
        .map_err(|_| "rcon timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if !(10..=MAX).contains(&len) {
        return Err("rcon: bad packet".into());
    }
    let mut buf = vec![0u8; len as usize];
    tokio::time::timeout(TIMEOUT, stream.read_exact(&mut buf))
        .await
        .map_err(|_| "rcon timeout".to_string())?
        .map_err(|e| e.to_string())?;
    let id = i32::from_le_bytes(buf[0..4].try_into().unwrap_or_default());
    let kind = i32::from_le_bytes(buf[4..8].try_into().unwrap_or_default());
    let body = String::from_utf8_lossy(&buf[8..buf.len() - 2]).to_string();
    Ok((id, kind, body))
}

impl Rcon {
    pub async fn connect(host: &str, port: u16, password: &str) -> Result<Self, String> {
        let stream = tokio::time::timeout(TIMEOUT, TcpStream::connect((host, port)))
            .await
            .map_err(|_| "rcon connect timeout".to_string())?
            .map_err(|e| format!("rcon connect: {e}"))?;
        let mut r = Rcon { stream, next: 1 };
        r.stream
            .write_all(&packet(r.next, AUTH, password))
            .await
            .map_err(|e| e.to_string())?;
        // Source servers send an empty RESPONSE_VALUE before the auth reply.
        for _ in 0..2 {
            let (id, kind, _) = read(&mut r.stream).await?;
            if kind == AUTH - 1 {
                if id == -1 {
                    return Err("rcon: wrong password".into());
                }
                r.next += 1;
                return Ok(r);
            }
        }
        Err("rcon: no auth reply".into())
    }

    pub async fn exec(&mut self, command: &str) -> Result<String, String> {
        if command.len() > 1000 || command.contains(['\r', '\n', '\0']) {
            return Err("rcon: invalid command".into());
        }
        let id = self.next;
        self.next += 1;
        self.stream
            .write_all(&packet(id, EXEC, command))
            .await
            .map_err(|e| e.to_string())?;
        let (rid, _, body) = read(&mut self.stream).await?;
        if rid != id {
            return Err("rcon: out of order reply".into());
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_layout() {
        let p = packet(7, EXEC, "list");
        assert_eq!(&p[0..4], &14i32.to_le_bytes());
        assert_eq!(&p[4..8], &7i32.to_le_bytes());
        assert_eq!(&p[8..12], &2i32.to_le_bytes());
        assert_eq!(&p[12..16], b"list");
        assert_eq!(&p[16..], &[0, 0]);
    }
}
