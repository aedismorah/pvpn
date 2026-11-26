use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use pvpn_core::ClientConfig;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "pvpn-client", about = "pvpn local SOCKS5 client")]
struct Args {
    #[arg(long, default_value = "client.toml")]
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
    let config = ClientConfig::from_toml(&text).context("parse client config")?;
    pvpn_client::client::run(config).await
}
