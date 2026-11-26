use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerEntry {
    pub name: String,
    pub addr: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Relative preference, 1.0 is neutral.
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_enabled() -> bool {
    true
}

fn default_weight() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub listen: String,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_ms: u64,
    #[serde(default = "default_probe_interval")]
    pub probe_interval_secs: u64,
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
}

fn default_connect_timeout() -> u64 {
    4000
}

fn default_probe_interval() -> u64 {
    15
}

impl RelayConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.servers.is_empty() {
            anyhow::bail!("relay config needs at least one [[servers]] entry");
        }
        for server in &self.servers {
            if server.name.trim().is_empty() {
                anyhow::bail!("server name must not be empty");
            }
            if !server.addr.contains(':') {
                anyhow::bail!("server {} addr must be host:port", server.name);
            }
            if !(server.weight.is_finite() && server.weight > 0.0) {
                anyhow::bail!("server {} weight must be positive", server.name);
            }
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        self.validate()?;
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        if std::fs::rename(&tmp, path).is_err() {
            // Windows cannot rename over an existing file.
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp, path)?;
        }
        Ok(())
    }

    pub fn upsert(&mut self, entry: ServerEntry) {
        if let Some(existing) = self
            .servers
            .iter_mut()
            .find(|server| server.name == entry.name)
        {
            *existing = entry;
        } else {
            self.servers.push(entry);
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.servers.len();
        self.servers.retain(|server| server.name != name);
        self.servers.len() != before
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        if let Some(server) = self.servers.iter_mut().find(|server| server.name == name) {
            server.enabled = enabled;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_and_edit() {
        let text = r#"
listen = "0.0.0.0:9331"
[[servers]]
name = "nl-1"
addr = "77.238.226.127:18443"
"#;
        let mut config: RelayConfig = toml::from_str(text).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert!(config.servers[0].enabled);
        config.upsert(ServerEntry {
            name: "nl-2".into(),
            addr: "77.238.226.128:18443".into(),
            enabled: true,
            weight: 1.0,
        });
        assert_eq!(config.servers.len(), 2);
        assert!(config.remove("nl-1"));
        assert_eq!(config.servers.len(), 1);
    }
}
