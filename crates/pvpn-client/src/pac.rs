use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

#[derive(Clone)]
pub struct PacState {
    domains: Arc<RwLock<Vec<String>>>,
    blacklist: Arc<AtomicBool>,
    proxy: Arc<RwLock<String>>,
}

impl PacState {
    pub fn new(proxy: impl Into<String>) -> Self {
        Self {
            domains: Arc::new(RwLock::new(Vec::new())),
            blacklist: Arc::new(AtomicBool::new(false)),
            proxy: Arc::new(RwLock::new(proxy.into())),
        }
    }

    pub fn set_proxy(&self, proxy: impl Into<String>) {
        if let Ok(mut guard) = self.proxy.write() {
            *guard = proxy.into();
        }
    }

    pub fn set_domains(&self, domains: Vec<String>) {
        if let Ok(mut guard) = self.domains.write() {
            *guard = domains;
        }
    }

    pub fn domains(&self) -> Vec<String> {
        self.domains.read().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn set_blacklist(&self, enabled: bool) {
        self.blacklist.store(enabled, Ordering::Relaxed);
    }

    pub fn blacklist(&self) -> bool {
        self.blacklist.load(Ordering::Relaxed)
    }

    pub fn pac(&self) -> Vec<u8> {
        let domains = serde_json::to_string(&self.domains()).unwrap_or_else(|_| "[]".into());
        let proxy = self
            .proxy
            .read()
            .map(|p| p.clone())
            .unwrap_or_else(|_| "127.0.0.1:11081".into())
            .replace('\\', "\\\\")
            .replace('\'', "\\'");
        let blacklist = if self.blacklist() { "true" } else { "false" };
        format!(
            r#"var sites = {domains};
var blacklist = {blacklist};
var proxy = "PROXY {proxy}";
function FindProxyForURL(url, host) {{
    host = host.toLowerCase().replace(/\.$/, "");
    if (host === "localhost" || host.indexOf(".") === -1 ||
        /\.local$/.test(host) || /^127\./.test(host) ||
        /^10\./.test(host) || /^192\.168\./.test(host) ||
        /^172\.(1[6-9]|2[0-9]|3[01])\./.test(host) ||
        /^169\.254\./.test(host)) return "DIRECT";
    var listed = false;
    for (var i = 0; i < sites.length; i++) {{
        var d = sites[i];
        if (host === d || host.slice(-(d.length + 1)) === "." + d) {{
            listed = true;
            break;
        }}
    }}
    return (blacklist ? !listed : listed) ? proxy : "DIRECT";
}}
"#
        )
        .into_bytes()
    }
}

pub async fn serve(
    listen: String,
    state: PacState,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&listen).await?;
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (socket, _) = accepted?;
                let state = state.clone();
                tokio::spawn(async move {
                    let _ = handle(socket, state).await;
                });
            }
        }
    }
    Ok(())
}

async fn handle(mut socket: TcpStream, state: PacState) -> anyhow::Result<()> {
    let mut buffer = [0u8; 4096];
    let n = socket.read(&mut buffer).await?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);

    let (status, content_type, body) = if path == "/proxy.pac" {
        (
            "200 OK",
            "application/x-ns-proxy-autoconfig",
            state.pac(),
        )
    } else if path == "/healthz" {
        ("200 OK", "text/plain; charset=utf-8", b"ok\n".to_vec())
    } else {
        ("404 Not Found", "text/plain; charset=utf-8", b"not found\n".to_vec())
    };

    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await?;
    socket.write_all(&body).await?;
    socket.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pac_contains_proxy_and_domains() {
        let state = PacState::new("127.0.0.1:11081");
        state.set_domains(vec!["chatgpt.com".into(), "youtube.com".into()]);
        let pac = String::from_utf8(state.pac()).unwrap();
        assert!(pac.contains("PROXY 127.0.0.1:11081"));
        assert!(pac.contains("chatgpt.com"));
        assert!(pac.contains("FindProxyForURL"));
    }

    #[test]
    fn blacklist_mode_changes_pac() {
        let state = PacState::new("127.0.0.1:11081");
        assert!(String::from_utf8(state.pac()).unwrap().contains("var blacklist = false"));
        state.set_blacklist(true);
        assert!(String::from_utf8(state.pac()).unwrap().contains("var blacklist = true"));
    }
}

