//! pvpn-core: custom encrypted transport for the pvpn tunnel.
//!
//! The crate is deliberately split into small modules:
//! - [`config`] parses TOML configuration.
//! - [`crypto`] implements the PSK handshake key schedule and frame AEAD.
//! - [`frame`] encodes/decodes frames and async wire I/O.
//! - [`handshake`] implements the padded hello exchange.
//!
//! See `docs/PROTOCOL.md` for the wire format.

pub mod config;
pub mod crypto;
pub mod frame;
pub mod handshake;

pub use config::{ClientConfig, Psk, ServerConfig};
pub use crypto::{FrameCipher, SessionKeys};
pub use frame::{read_frame, write_frame, Frame, FrameError, TargetAddr};
pub use handshake::{client_handshake, server_handshake, HandshakeError};
