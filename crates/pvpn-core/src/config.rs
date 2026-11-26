use std::fmt;

use serde::de::{self, Deserializer};
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// 32-byte pre-shared key. `Debug` is redacted.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Psk([u8; 32]);

impl Psk {
    pub fn from_hex(value: &str) -> Result<Self, ConfigError> {
        let bytes = hex::decode(value.trim()).map_err(|_| ConfigError::BadPsk)?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| ConfigError::BadPsk)?;
        Ok(Self(arr))
    }

    pub fn generate() -> Self {
        use rand::RngCore;
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self(key)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Psk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Psk(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for Psk {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Psk::from_hex(&value).map_err(de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("psk_hex must be exactly 64 hex characters (32 bytes)")]
    BadPsk,
    #[error("invalid configuration: {0}")]
    Toml(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Listen address, e.g. `0.0.0.0:18443`.
    pub listen: String,
    pub psk: Psk,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_max_frame")]
    pub max_frame: usize,
    /// Allow targets in loopback/private/link-local ranges. Keep false in
    /// production; enable only for local smoke tests.
    #[serde(default)]
    pub allow_private_targets: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientConfig {
    /// Loopback SOCKS5 listen address, e.g. `127.0.0.1:11080`.
    pub listen: String,
    /// Server ingress, e.g. `cloud.example:443`.
    pub server: String,
    pub psk: Psk,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    /// Optional HTTP/CONNECT proxy inbound. The PAC points here.
    #[serde(default)]
    pub http_listen: Option<String>,
}

fn default_connect_timeout() -> u64 {
    15
}

fn default_max_frame() -> usize {
    65_535
}

impl ServerConfig {
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }
}

impl ClientConfig {
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_and_redacts_psk() {
        let psk = "11".repeat(32);
        let text = format!(
            r#"
listen = "127.0.0.1:11080"
server = "127.0.0.1:18443"
psk = "{psk}"
"#
        );
        let cfg = ClientConfig::from_toml(&text).unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:11080");
        assert!(!format!("{:?}", cfg).contains(&psk));
    }

    #[test]
    fn rejects_short_psk() {
        let text = r#"
listen = "127.0.0.1:11080"
server = "127.0.0.1:18443"
psk = "abcd"
"#;
        assert!(ClientConfig::from_toml(text).is_err());
    }
}
