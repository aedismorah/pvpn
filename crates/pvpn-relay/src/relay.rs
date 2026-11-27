use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use rand::Rng;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::{timeout, Instant};
use tracing::{debug, info, warn};

use crate::config::{RelayConfig, ServerEntry};

#[derive(Debug, Clone)]
struct RuntimeServer {
    entry: ServerEntry,
    latency_ms: f64,
    throughput_bps: f64,
    healthy: bool,
    failures: u32,
}

impl RuntimeServer {
    fn new(entry: ServerEntry) -> Self {
        Self {
            entry,
            latency_ms: 100.0,
            throughput_bps: 0.0,
            healthy: true,
            failures: 0,
        }
    }

    fn score(&self) -> f64 {
        let latency = self.latency_ms.max(1.0);
        let kbps = if self.throughput_bps > 0.0 {
            self.throughput_bps / 1024.0
        } else {
            500.0
        };
        let throughput_penalty = 100_000.0 / kbps.max(1.0);
        let jitter = rand::thread_rng().gen_range(0.0..25.0);
        (latency + throughput_penalty + jitter) / self.entry.weight
    }
}

struct Shared {
    servers: RwLock<Vec<RuntimeServer>>,
    connect_timeout: Duration,
}

impl Shared {
    async fn pick(&self, exclude: &HashSet<String>) -> Option<(String, String)> {
        let servers = self.servers.read().await;
        let mut candidates: Vec<&RuntimeServer> = servers
            .iter()
            .filter(|s| s.entry.enabled && s.healthy && !exclude.contains(&s.entry.name))
            .collect();
        if candidates.is_empty() {
            candidates = servers
                .iter()
                .filter(|s| s.entry.enabled && !exclude.contains(&s.entry.name))
                .collect();
        }
        candidates
            .into_iter()
            .min_by(|a, b| a.score().total_cmp(&b.score()))
            .map(|s| (s.entry.name.clone(), s.entry.addr.clone()))
    }

    async fn mark_failure(&self, name: &str) {
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.iter_mut().find(|s| s.entry.name == name) {
            server.failures = server.failures.saturating_add(1);
            if server.failures >= 3 {
                server.healthy = false;
                warn!(server = name, "exit marked unhealthy");
            }
        }
    }

    async fn record_probe(&self, name: &str, latency: Option<Duration>) {
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.iter_mut().find(|s| s.entry.name == name) {
            match latency {
                Some(latency) => {
                    let ms = latency.as_secs_f64() * 1000.0;
                    server.latency_ms = if server.latency_ms <= 0.0 {
                        ms
                    } else {
                        server.latency_ms * 0.7 + ms * 0.3
                    };
                    server.healthy = true;
                    server.failures = 0;
                }
                None => {
                    server.failures = server.failures.saturating_add(1);
                    if server.failures >= 3 {
                        server.healthy = false;
                    }
                }
            }
        }
    }

    async fn record_throughput(&self, name: &str, bytes: u64, elapsed: Duration) {
        if bytes == 0 || elapsed.is_zero() {
            return;
        }
        let bps = bytes as f64 / elapsed.as_secs_f64();
        let mut servers = self.servers.write().await;
        if let Some(server) = servers.iter_mut().find(|s| s.entry.name == name) {
            server.throughput_bps = if server.throughput_bps <= 0.0 {
                bps
            } else {
                server.throughput_bps * 0.75 + bps * 0.25
            };
        }
    }

    async fn snapshot(&self) -> Vec<(String, String)> {
        let servers = self.servers.read().await;
        servers
            .iter()
            .filter(|s| s.entry.enabled)
            .map(|s| (s.entry.name.clone(), s.entry.addr.clone()))
            .collect()
    }

    #[allow(dead_code)]
    async fn status_rows(&self) -> Vec<String> {
        let servers = self.servers.read().await;
        servers
            .iter()
            .map(|s| {
                format!(
                    "{}\t{}\tenabled={}\thealthy={}\t{:.1}ms\t{:.0}KB/s",
                    s.entry.name,
                    s.entry.addr,
                    s.entry.enabled,
                    s.healthy,
                    s.latency_ms,
                    s.throughput_bps / 1024.0
                )
            })
            .collect()
    }
}

pub async fn run(config_path: PathBuf, config: RelayConfig) -> anyhow::Result<()> {
    let shared = Arc::new(Shared {
        servers: RwLock::new(
            config
                .servers
                .iter()
                .cloned()
                .map(RuntimeServer::new)
                .collect(),
        ),
        connect_timeout: Duration::from_millis(config.connect_timeout_ms.max(100)),
    });

    // Probe once before accepting so the first connection has a real ranking.
    probe_once(&shared).await;

    let listener = TcpListener::bind(&config.listen).await?;
    info!(
        listen = %config.listen,
        servers = config.servers.len(),
        "pvpn-relay listening"
    );

    let probe_shared = shared.clone();
    let interval = Duration::from_secs(config.probe_interval_secs.max(3));
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            probe_once(&probe_shared).await;
        }
    });

    let watch_shared = shared.clone();
    let watch_path = config_path.clone();
    tokio::spawn(async move {
        watch_config(watch_shared, watch_path).await;
    });

    loop {
        let (downstream, peer) = listener.accept().await?;
        let shared = shared.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(downstream, shared).await {
                debug!(%peer, error = %error, "relay connection ended");
            }
        });
    }
}

async fn handle_connection(mut downstream: TcpStream, shared: Arc<Shared>) -> anyhow::Result<()> {
    downstream.set_nodelay(true)?;

    let mut tried = HashSet::new();
    for _ in 0..3 {
        let Some((name, addr)) = shared.pick(&tried).await else {
            break;
        };
        tried.insert(name.clone());
        let connect = async {
            let stream = TcpStream::connect(addr.as_str()).await?;
            stream.set_nodelay(true)?;
            Ok::<_, std::io::Error>(stream)
        };

        match timeout(shared.connect_timeout, connect).await {
            Ok(Ok(mut upstream)) => {
                debug!(server = %name, "relay selected exit");
                let started = Instant::now();
                let result = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
                let elapsed = started.elapsed();
                match result {
                    Ok((to_exit, from_exit)) => {
                        shared
                            .record_throughput(&name, to_exit + from_exit, elapsed)
                            .await;
                    }
                    Err(error) => {
                        debug!(server = %name, error = %error, "relay copy failed");
                        shared.mark_failure(&name).await;
                    }
                }
                return Ok(());
            }
            _ => {
                debug!(server = %name, "exit connect failed, trying another");
                shared.mark_failure(&name).await;
            }
        }
    }

    anyhow::bail!("no healthy exit available")
}

async fn probe_once(shared: &Shared) {
    for (name, addr) in shared.snapshot().await {
        let started = Instant::now();
        let result = timeout(Duration::from_secs(2), TcpStream::connect(addr.as_str())).await;
        let latency = match result {
            Ok(Ok(_)) => Some(started.elapsed()),
            _ => None,
        };
        shared.record_probe(&name, latency).await;
    }
}

async fn watch_config(shared: Arc<Shared>, path: PathBuf) {
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        let loaded = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))
            .and_then(|text| toml::from_str::<RelayConfig>(&text).map_err(anyhow::Error::from));
        let config = match loaded {
            Ok(config) if config.validate().is_ok() => config,
            Ok(_) => {
                warn!(path = %path.display(), "ignored invalid relay config");
                continue;
            }
            Err(error) => {
                warn!(path = %path.display(), error = %error, "failed to reload relay config");
                continue;
            }
        };

        let mut servers = shared.servers.write().await;
        let current: Vec<ServerEntry> = servers.iter().map(|s| s.entry.clone()).collect();
        if current != config.servers {
            let mut old: HashMap<String, RuntimeServer> = servers
                .drain(..)
                .map(|server| (server.entry.name.clone(), server))
                .collect();
            let mut next = Vec::with_capacity(config.servers.len());
            for entry in &config.servers {
                if let Some(mut existing) = old.remove(&entry.name) {
                    existing.entry = entry.clone();
                    next.push(existing);
                } else {
                    next.push(RuntimeServer::new(entry.clone()));
                }
            }
            *servers = next;
            info!(count = config.servers.len(), "relay server list reloaded");
        }
    }
}
