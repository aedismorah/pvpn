use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use pvpn_core::ServerConfig;
use tracing_subscriber::EnvFilter;

mod server;

#[derive(Debug, Parser)]
#[command(name = "pvpn-server", about = "pvpn exit server")]
struct Args {
    #[arg(long, default_value = "server.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();
    let text = std::fs::read_to_string(&args.config)
        .with_context(|| format!("read {}", args.config.display()))?;
    let config = ServerConfig::from_toml(&text).context("parse server config")?;
    server::run(config).await
}
