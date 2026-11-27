use std::net::SocketAddr;
use std::sync::Arc;

use pvpn_core::TargetAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

use crate::client::{open_session, relay_with_initial};

const MAX_HEADER: usize = 16 * 1024;

/// Minimal HTTP proxy: supports CONNECT (HTTPS) and absolute-URI HTTP requests.
/// This is what the Windows PAC points at (`PROXY 127.0.0.1:11081`).
pub async fn handle(mut local: TcpStream, config: Arc<pvpn_core::ClientConfig>) -> anyhow::Result<()> {
    local.set_nodelay(true)?;
    let mut buffer = Vec::with_capacity(1024);
    let header_end = loop {
        let mut chunk = [0u8; 1024];
        let n = local.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buffer) {
            break pos;
        }
        if buffer.len() > MAX_HEADER {
            anyhow::bail!("http header too large");
        }
    };

    let header = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let uri = parts.next().unwrap_or_default().to_string();

    if method.eq_ignore_ascii_case("CONNECT") {
        let Some((host, port)) = parse_authority(&uri) else {
            return bad_gateway(&mut local).await;
        };
        let target = target_for(&host, port);
        match open_session(&config, &target).await {
            Ok(session) => {
                local
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await?;
                local.flush().await?;
                relay_with_initial(&mut local, session, &[]).await?;
            }
            Err(error) => {
                debug!(error = %error, "http CONNECT open failed");
                bad_gateway(&mut local).await?;
            }
        }
        return Ok(());
    }

    let Some(rest) = uri.strip_prefix("http://") else {
        local
            .write_all(b"HTTP/1.1 501 Not Implemented\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return Ok(());
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let Some((host, port)) = parse_authority(authority).or_else(|| Some((authority.to_string(), 80))) else {
        return bad_gateway(&mut local).await;
    };
    let target = target_for(&host, port);

    match open_session(&config, &target).await {
        Ok(session) => {
            let mut forwarded = format!("{method} {path} HTTP/1.1\r\n");
            for line in lines {
                if line.is_empty()
                    || line.to_ascii_lowercase().starts_with("proxy-connection:")
                    || line.to_ascii_lowercase().starts_with("connection:")
                {
                    continue;
                }
                forwarded.push_str(line);
                forwarded.push_str("\r\n");
            }
            forwarded.push_str("Connection: close\r\n\r\n");
            let mut initial = forwarded.into_bytes();
            initial.extend_from_slice(&buffer[header_end + 4..]);
            relay_with_initial(&mut local, session, &initial).await?;
        }
        Err(error) => {
            debug!(error = %error, "http request open failed");
            bad_gateway(&mut local).await?;
        }
    }
    Ok(())
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_authority(value: &str) -> Option<(String, u16)> {
    if let Some(rest) = value.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = tail.strip_prefix(':')?.parse().ok()?;
        return Some((host.to_string(), port));
    }
    let (host, port) = value.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port.parse().ok()?))
}

fn target_for(host: &str, port: u16) -> TargetAddr {
    match format!("{host}:{port}").parse::<SocketAddr>() {
        Ok(addr) => TargetAddr::Ip(addr),
        Err(_) => TargetAddr::Domain(host.to_string(), port),
    }
}

async fn bad_gateway(local: &mut TcpStream) -> anyhow::Result<()> {
    local
        .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
        .await?;
    local.flush().await?;
    Ok(())
}
