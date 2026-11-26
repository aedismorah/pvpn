use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use pvpn_core::TargetAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SOCKS_VERSION: u8 = 0x05;
const CMD_CONNECT: u8 = 0x01;
const CMD_UDP_ASSOCIATE: u8 = 0x03;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;

pub enum Request {
    Connect(TargetAddr),
    UdpAssociate,
}

pub async fn handshake(local: &mut TcpStream) -> io::Result<Request> {
    let mut head = [0u8; 2];
    local.read_exact(&mut head).await?;
    if head[0] != SOCKS_VERSION {
        return Err(ErrorKind::InvalidData.into());
    }
    let mut methods = vec![0u8; head[1] as usize];
    local.read_exact(&mut methods).await?;
    if !methods.contains(&0x00) {
        local.write_all(&[SOCKS_VERSION, 0xFF]).await?;
        return Err(ErrorKind::PermissionDenied.into());
    }
    local.write_all(&[SOCKS_VERSION, 0x00]).await?;

    let mut request = [0u8; 4];
    local.read_exact(&mut request).await?;
    if request[0] != SOCKS_VERSION {
        return Err(ErrorKind::InvalidData.into());
    }
    let command = request[1];
    let atyp = request[3];
    let target = read_target(local, atyp).await?;
    match command {
        CMD_CONNECT => Ok(Request::Connect(target)),
        CMD_UDP_ASSOCIATE => Ok(Request::UdpAssociate),
        _ => {
            reply(local, 0x07).await?;
            Err(ErrorKind::Unsupported.into())
        }
    }
}

async fn read_target(local: &mut TcpStream, atyp: u8) -> io::Result<TargetAddr> {
    let ip = match atyp {
        ATYP_IPV4 => {
            let mut octets = [0u8; 4];
            local.read_exact(&mut octets).await?;
            Some(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        ATYP_IPV6 => {
            let mut octets = [0u8; 16];
            local.read_exact(&mut octets).await?;
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        ATYP_DOMAIN => {
            let mut len = [0u8; 1];
            local.read_exact(&mut len).await?;
            let mut host = vec![0u8; len[0] as usize];
            local.read_exact(&mut host).await?;
            let host = String::from_utf8(host).map_err(|_| ErrorKind::InvalidData)?;
            if host.is_empty() {
                return Err(ErrorKind::InvalidData.into());
            }
            let mut port = [0u8; 2];
            local.read_exact(&mut port).await?;
            return Ok(TargetAddr::Domain(host, u16::from_be_bytes(port)));
        }
        _ => {
            return Err(ErrorKind::InvalidData.into());
        }
    };

    let mut port = [0u8; 2];
    local.read_exact(&mut port).await?;
    Ok(TargetAddr::Ip(SocketAddr::new(
        ip.expect("ip set above"),
        u16::from_be_bytes(port),
    )))
}

pub async fn reply_ok(local: &mut TcpStream) -> io::Result<()> {
    reply(local, 0x00).await
}

pub async fn reply_udp(local: &mut TcpStream, port: u16) -> io::Result<()> {
    let mut response = vec![SOCKS_VERSION, 0x00, 0x00, ATYP_IPV4, 127, 0, 0, 1];
    response.extend_from_slice(&port.to_be_bytes());
    local.write_all(&response).await?;
    local.flush().await?;
    Ok(())
}

pub fn parse_udp(packet: &[u8]) -> Option<(TargetAddr, &[u8])> {
    if packet.len() < 4 || packet[0] != 0 || packet[1] != 0 || packet[2] != 0 {
        return None;
    }
    let atyp = packet[3];
    let (target, offset) = match atyp {
        ATYP_IPV4 => {
            if packet.len() < 10 {
                return None;
            }
            let ip = Ipv4Addr::new(packet[4], packet[5], packet[6], packet[7]);
            let port = u16::from_be_bytes([packet[8], packet[9]]);
            (TargetAddr::Ip(SocketAddr::new(IpAddr::V4(ip), port)), 10)
        }
        ATYP_IPV6 => {
            if packet.len() < 22 {
                return None;
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&packet[4..20]);
            let port = u16::from_be_bytes([packet[20], packet[21]]);
            (
                TargetAddr::Ip(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(octets)), port)),
                22,
            )
        }
        ATYP_DOMAIN => {
            if packet.len() < 5 {
                return None;
            }
            let len = packet[4] as usize;
            if packet.len() < 5 + len + 2 {
                return None;
            }
            let host = String::from_utf8(packet[5..5 + len].to_vec()).ok()?;
            let port = u16::from_be_bytes([packet[5 + len], packet[5 + len + 1]]);
            (TargetAddr::Domain(host, port), 5 + len + 2)
        }
        _ => return None,
    };
    Some((target, &packet[offset..]))
}

pub fn encode_udp(target: &TargetAddr, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 24);
    out.extend_from_slice(&[0, 0, 0]);
    match target {
        TargetAddr::Ip(SocketAddr::V4(addr)) => {
            out.push(ATYP_IPV4);
            out.extend_from_slice(&addr.ip().octets());
            out.extend_from_slice(&addr.port().to_be_bytes());
        }
        TargetAddr::Ip(SocketAddr::V6(addr)) => {
            out.push(ATYP_IPV6);
            out.extend_from_slice(&addr.ip().octets());
            out.extend_from_slice(&addr.port().to_be_bytes());
        }
        TargetAddr::Domain(host, port) => {
            let bytes = host.as_bytes();
            let len = bytes.len().min(255);
            out.push(ATYP_DOMAIN);
            out.push(len as u8);
            out.extend_from_slice(&bytes[..len]);
            out.extend_from_slice(&port.to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

pub async fn reply_failure(local: &mut TcpStream) -> io::Result<()> {
    reply(local, 0x01).await
}

async fn reply(local: &mut TcpStream, code: u8) -> io::Result<()> {
    let response = [SOCKS_VERSION, code, 0x00, ATYP_IPV4, 0, 0, 0, 0, 0, 0];
    local.write_all(&response).await?;
    local.flush().await?;
    Ok(())
}
