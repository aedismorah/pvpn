use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use rand::Rng;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::crypto::FrameCipher;

pub mod frame_type {
    pub const OPEN_TCP: u8 = 0x01;
    pub const DATA: u8 = 0x02;
    pub const CLOSE: u8 = 0x03;
    pub const PING: u8 = 0x04;
    pub const PONG: u8 = 0x05;
    pub const PADDING: u8 = 0x06;
    pub const OPEN_RESULT: u8 = 0x07;
    pub const ERROR: u8 = 0x08;
    pub const OPEN_UDP: u8 = 0x09;
    pub const UDP_DATA: u8 = 0x0A;
    pub const SHUT_WR: u8 = 0x0B;
}

pub mod open_status {
    pub const OK: u8 = 0;
    pub const DNS: u8 = 1;
    pub const CONNECT: u8 = 2;
    pub const TIMEOUT: u8 = 3;
    pub const POLICY: u8 = 4;
}

const MAX_BODY: usize = 65_535 - 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetAddr {
    Ip(SocketAddr),
    Domain(String, u16),
}

impl TargetAddr {
    pub fn domain(host: impl Into<String>, port: u16) -> Self {
        Self::Domain(host.into(), port)
    }

    pub fn socket_addr(&self) -> Option<SocketAddr> {
        match self {
            Self::Ip(addr) => Some(*addr),
            Self::Domain(_, _) => None,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        match self {
            Self::Ip(SocketAddr::V4(addr)) => {
                out.push(1);
                out.extend_from_slice(&addr.ip().octets());
            }
            Self::Ip(SocketAddr::V6(addr)) => {
                out.push(3);
                out.extend_from_slice(&addr.ip().octets());
            }
            Self::Domain(host, _) => {
                let bytes = host.as_bytes();
                if bytes.is_empty() || bytes.len() > 255 {
                    return Err(FrameError::BadBody("domain length"));
                }
                out.push(2);
                out.push(bytes.len() as u8);
                out.extend_from_slice(bytes);
            }
        }
        let port = match self {
            Self::Ip(addr) => addr.port(),
            Self::Domain(_, port) => *port,
        };
        out.extend_from_slice(&port.to_be_bytes());
        Ok(())
    }

    fn decode(kind: u8, addr: &[u8], port: u16) -> Result<Self, FrameError> {
        match kind {
            1 => {
                let octets: [u8; 4] = addr
                    .try_into()
                    .map_err(|_| FrameError::BadBody("ipv4 length"))?;
                Ok(Self::Ip(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::from(octets)),
                    port,
                )))
            }
            3 => {
                let octets: [u8; 16] = addr
                    .try_into()
                    .map_err(|_| FrameError::BadBody("ipv6 length"))?;
                Ok(Self::Ip(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(octets)),
                    port,
                )))
            }
            2 => {
                let host = std::str::from_utf8(addr)
                    .map_err(|_| FrameError::BadBody("domain utf8"))?
                    .to_owned();
                if host.is_empty() {
                    return Err(FrameError::BadBody("empty domain"));
                }
                Ok(Self::Domain(host, port))
            }
            _ => Err(FrameError::BadBody("address type")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub typ: u8,
    pub stream_id: u32,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn new(typ: u8, stream_id: u32, body: Vec<u8>) -> Self {
        Self {
            typ,
            stream_id,
            body,
        }
    }

    pub fn open_tcp(stream_id: u32, target: &TargetAddr) -> Result<Self, FrameError> {
        let mut body = Vec::with_capacity(24);
        target.encode(&mut body)?;
        Ok(Self::new(frame_type::OPEN_TCP, stream_id, body))
    }

    pub fn open_udp(stream_id: u32) -> Self {
        Self::new(frame_type::OPEN_UDP, stream_id, Vec::new())
    }

    pub fn udp_data(
        stream_id: u32,
        target: &TargetAddr,
        data: &[u8],
    ) -> Result<Self, FrameError> {
        let mut body = Vec::with_capacity(data.len() + 24);
        target.encode(&mut body)?;
        body.extend_from_slice(data);
        Ok(Self::new(frame_type::UDP_DATA, stream_id, body))
    }

    pub fn data(stream_id: u32, data: Vec<u8>) -> Self {
        let mut body = Vec::with_capacity(2 + data.len());
        body.extend_from_slice(&(data.len() as u16).to_be_bytes());
        body.extend_from_slice(&data);
        Self::new(frame_type::DATA, stream_id, body)
    }

    /// Payload padded to a coarse bucket to blur application fingerprints.
    pub fn data_padded(stream_id: u32, data: Vec<u8>) -> Self {
        if data.len() > 1400 {
            return Self::data(stream_id, data);
        }
        let bucket: usize = match data.len() {
            0..=200 => 256,
            201..=456 => 512,
            457..=968 => 1024,
            _ => 1500,
        };
        let pad = bucket.saturating_sub(2 + data.len());
        let mut body = Vec::with_capacity(bucket);
        body.extend_from_slice(&(data.len() as u16).to_be_bytes());
        body.extend_from_slice(&data);
        if pad > 0 {
            let mut filler = vec![0u8; pad];
            rand::thread_rng().fill(&mut filler[..]);
            body.extend_from_slice(&filler);
        }
        Self::new(frame_type::DATA, stream_id, body)
    }

    pub fn close(stream_id: u32, reason: u8) -> Self {
        Self::new(frame_type::CLOSE, stream_id, vec![reason])
    }

    /// Half-close the client-to-target direction without closing the target-to-client
    /// direction. This is used when the local application shuts down its write side.
    pub fn shutdown_write(stream_id: u32) -> Self {
        Self::new(frame_type::SHUT_WR, stream_id, Vec::new())
    }

    pub fn ping(stream_id: u32) -> Self {
        let mut payload = vec![0u8; 8];
        rand::thread_rng().fill(&mut payload[..]);
        Self::new(frame_type::PING, stream_id, payload)
    }

    pub fn pong(stream_id: u32, payload: Vec<u8>) -> Self {
        Self::new(frame_type::PONG, stream_id, payload)
    }

    pub fn padding(len: usize) -> Self {
        let mut body = vec![0u8; len.min(1024)];
        rand::thread_rng().fill(&mut body[..]);
        Self::new(frame_type::PADDING, 0, body)
    }

    pub fn open_result(stream_id: u32, status: u8) -> Self {
        Self::new(frame_type::OPEN_RESULT, stream_id, vec![status])
    }

    pub fn error(stream_id: u32, code: u16, message: impl Into<String>) -> Self {
        let message = message.into();
        let mut body = Vec::with_capacity(2 + message.len());
        body.extend_from_slice(&code.to_be_bytes());
        body.extend_from_slice(message.as_bytes());
        Self::new(frame_type::ERROR, stream_id, body)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.body.len());
        out.push(self.typ);
        out.extend_from_slice(&self.stream_id.to_be_bytes());
        out.extend_from_slice(&self.body);
        out
    }

    pub fn decode(plain: &[u8]) -> Result<Self, FrameError> {
        if plain.len() < 5 {
            return Err(FrameError::Truncated);
        }
        let typ = plain[0];
        let stream_id = u32::from_be_bytes([plain[1], plain[2], plain[3], plain[4]]);
        let body = plain[5..].to_vec();
        if body.len() > MAX_BODY {
            return Err(FrameError::TooLarge(body.len()));
        }
        match typ {
            frame_type::OPEN_TCP
            | frame_type::DATA
            | frame_type::CLOSE
            | frame_type::PING
            | frame_type::PONG
            | frame_type::PADDING
            | frame_type::OPEN_RESULT
            | frame_type::ERROR
            | frame_type::OPEN_UDP
            | frame_type::UDP_DATA
            | frame_type::SHUT_WR => Ok(Self {
                typ,
                stream_id,
                body,
            }),
            other => Err(FrameError::BadType(other)),
        }
    }

    pub fn decode_open_tcp(&self) -> Result<TargetAddr, FrameError> {
        if self.typ != frame_type::OPEN_TCP || self.body.is_empty() {
            return Err(FrameError::BadBody("not OPEN_TCP"));
        }
        let kind = self.body[0];
        let (addr, rest) = match kind {
            1 => {
                if self.body.len() < 1 + 4 + 2 {
                    return Err(FrameError::Truncated);
                }
                (&self.body[1..5], &self.body[5..])
            }
            3 => {
                if self.body.len() < 1 + 16 + 2 {
                    return Err(FrameError::Truncated);
                }
                (&self.body[1..17], &self.body[17..])
            }
            2 => {
                if self.body.len() < 2 {
                    return Err(FrameError::Truncated);
                }
                let len = self.body[1] as usize;
                if self.body.len() < 2 + len + 2 {
                    return Err(FrameError::Truncated);
                }
                (&self.body[2..2 + len], &self.body[2 + len..])
            }
            _ => return Err(FrameError::BadBody("address type")),
        };
        let port = u16::from_be_bytes([rest[0], rest[1]]);
        TargetAddr::decode(kind, addr, port)
    }

    pub fn decode_udp_data(&self) -> Result<(TargetAddr, &[u8]), FrameError> {
        if self.typ != frame_type::UDP_DATA || self.body.is_empty() {
            return Err(FrameError::BadBody("not UDP_DATA"));
        }
        let kind = self.body[0];
        let (addr_len, addr_start) = match kind {
            1 => (4, 1),
            3 => (16, 1),
            2 => {
                if self.body.len() < 2 {
                    return Err(FrameError::Truncated);
                }
                (self.body[1] as usize, 2)
            }
            _ => return Err(FrameError::BadBody("address type")),
        };
        let port_start = addr_start + addr_len;
        let data_start = port_start + 2;
        if self.body.len() < data_start {
            return Err(FrameError::Truncated);
        }
        let port = u16::from_be_bytes([self.body[port_start], self.body[port_start + 1]]);
        let target = TargetAddr::decode(
            kind,
            &self.body[addr_start..port_start],
            port,
        )?;
        Ok((target, &self.body[data_start..]))
    }

    pub fn decode_data(&self) -> Result<&[u8], FrameError> {
        if self.typ != frame_type::DATA || self.body.len() < 2 {
            return Err(FrameError::BadBody("not DATA"));
        }
        let len = u16::from_be_bytes([self.body[0], self.body[1]]) as usize;
        if self.body.len() < 2 + len {
            return Err(FrameError::Truncated);
        }
        Ok(&self.body[2..2 + len])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("truncated frame")]
    Truncated,
    #[error("bad frame length: {0}")]
    BadLength(usize),
    #[error("frame too large: {0}")]
    TooLarge(usize),
    #[error("frame authentication failed")]
    Crypto,
    #[error("frame sequence exhausted")]
    SequenceExhausted,
    #[error("unknown frame type: {0:#x}")]
    BadType(u8),
    #[error("bad frame body: {0}")]
    BadBody(&'static str),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    cipher: &mut FrameCipher,
    frame: &Frame,
) -> Result<(), FrameError> {
    let wire = cipher.seal(frame)?;
    writer.write_all(&wire).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
    cipher: &mut FrameCipher,
    max_frame: usize,
) -> Result<Frame, FrameError> {
    let mut header = [0u8; 4];
    reader.read_exact(&mut header).await?;
    let len = cipher.encrypted_len(&header)?;
    if len > max_frame {
        return Err(FrameError::TooLarge(len));
    }
    let mut wire = vec![0u8; 4 + len];
    wire[..4].copy_from_slice(&header);
    reader.read_exact(&mut wire[4..]).await?;
    cipher.open(&wire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_tcp_roundtrip() {
        for target in [
            TargetAddr::Ip("1.2.3.4:443".parse().unwrap()),
            TargetAddr::Ip("[2001:db8::1]:443".parse().unwrap()),
            TargetAddr::Domain("example.com".into(), 8443),
        ] {
            let frame = Frame::open_tcp(7, &target).unwrap();
            assert_eq!(frame.decode_open_tcp().unwrap(), target);
        }
    }

    #[test]
    fn data_padding_keeps_payload() {
        let frame = Frame::data_padded(1, b"abc".to_vec());
        assert_eq!(frame.body.len(), 256);
        assert_eq!(frame.decode_data().unwrap(), b"abc");
    }

    #[test]
    fn udp_data_roundtrip() {
        let target = TargetAddr::Domain("quic.example".into(), 443);
        let frame = Frame::udp_data(9, &target, b"datagram").unwrap();
        let (decoded, payload) = frame.decode_udp_data().unwrap();
        assert_eq!(decoded, target);
        assert_eq!(payload, b"datagram");
    }
}
