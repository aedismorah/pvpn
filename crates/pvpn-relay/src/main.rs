use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod config;
mod relay;

use config::{RelayConfig, ServerEntry};

#[derive(Debug, Parser)]
#[command(name = "pvpn-relay", about = "pvpn smart TCP relay")]
struct Cli {
    #[arg(long, global = true, default_value = "/etc/pvpn/relay.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the relay (default).
    Run,
    /// Add or update an exit server.
    Add {
        name: String,
        addr: String,
        #[arg(long, default_value_t = 1.0)]
        weight: f64,
        #[arg(long)]
        disabled: bool,
    },
    /// Remove an exit server.
    Remove { name: String },
    /// Enable an exit server.
    Enable { name: String },
    /// Disable an exit server.
    Disable { name: String },
    /// List configured exit servers.
    List,
    /// TCP-probe every configured exit and print latency.
    Probe,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::Run);
    match command {
        Command::Run => {
            let config = RelayConfig::load(&cli.config)
                .with_context(|| format!("load {}", cli.config.display()))?;
            relay::run(cli.config, config).await
        }
        Command::Add {
            name,
            addr,
            weight,
            disabled,
        } => {
            let mut config = RelayConfig::load(&cli.config)?;
            config.upsert(ServerEntry {
                name: name.clone(),
                addr: addr.clone(),
                enabled: !disabled,
                weight,
            });
            config.save(&cli.config)?;
            println!("added {name} -> {addr}");
            Ok(())
        }
        Command::Remove { name } => {
            let mut config = RelayConfig::load(&cli.config)?;
            if config.remove(&name) {
                config.save(&cli.config)?;
                println!("removed {name}");
            } else {
                anyhow::bail!("server {name} not found");
            }
            Ok(())
        }
        Command::Enable { name } => {
            let mut config = RelayConfig::load(&cli.config)?;
            if config.set_enabled(&name, true) {
                config.save(&cli.config)?;
                println!("enabled {name}");
                Ok(())
            } else {
                anyhow::bail!("server {name} not found")
            }
        }
        Command::Disable { name } => {
            let mut config = RelayConfig::load(&cli.config)?;
            if config.set_enabled(&name, false) {
                config.save(&cli.config)?;
                println!("disabled {name}");
                Ok(())
            } else {
                anyhow::bail!("server {name} not found")
            }
        }
        Command::List => {
            let config = RelayConfig::load(&cli.config)?;
            for server in config.servers {
                println!(
                    "{}\t{}\tenabled={}\tweight={}",
                    server.name, server.addr, server.enabled, server.weight
                );
            }
            Ok(())
        }
        Command::Probe => {
            let config = RelayConfig::load(&cli.config)?;
            for server in config.servers {
                let started = Instant::now();
                let result = tokio::time::timeout(
                    Duration::from_secs(3),
                    tokio::net::TcpStream::connect(server.addr.as_str()),
                )
                .await;
                match result {
                    Ok(Ok(_)) => println!(
                        "{}\t{}\t{:.1} ms",
                        server.name,
                        server.addr,
                        started.elapsed().as_secs_f64() * 1000.0
                    ),
                    _ => println!("{}\t{}\tunreachable", server.name, server.addr),
                }
            }
            Ok(())
        }
    }
}
