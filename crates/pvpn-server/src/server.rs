use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pvpn_core::frame::{frame_type, open_status};
use pvpn_core::{read_frame, server_handshake, write_frame, Frame, ServerConfig, TargetAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, warn};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&config.listen).await?;
    info!(listen = %config.listen, "pvpn-server listening");
    let config = Arc::new(config);
    loop {
        let (socket, peer) = listener.accept().await?;
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(socket, config).await {
                debug!(%peer, error = %error, "connection closed");
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, config: Arc<ServerConfig>) -> anyhow::Result<()> {
    socket.set_nodelay(true)?;
    let (send, mut recv) = timeout(
        HANDSHAKE_TIMEOUT,
        server_handshake(&mut socket, &config.psk),
    )
    .await
    .map_err(|_| anyhow::anyhow!("handshake timeout"))??;

    let (mut reader, mut writer) = socket.into_split();
    let (tx, mut rx) = mpsc::channel::<Frame>(256);

    let writer_task = tokio::spawn(async move {
        let mut send = send;
        while let Some(frame) = rx.recv().await {
            if write_frame(&mut writer, &mut send, &frame).await.is_err() {
                break;
            }
        }
    });

    let mut streams: HashMap<u32, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut udp_streams: HashMap<u32, mpsc::Sender<(TargetAddr, Vec<u8>)>> = HashMap::new();
    loop {
        let frame = match read_frame(&mut reader, &mut recv, config.max_frame).await {
            Ok(frame) => frame,
            Err(error) => {
                debug!(error = %error, "read frame failed");
                break;
            }
        };

        match frame.typ {
            frame_type::OPEN_TCP => {
                let target = match frame.decode_open_tcp() {
                    Ok(target) => target,
                    Err(error) => {
                        let _ = tx
                            .send(Frame::error(frame.stream_id, 400, error.to_string()))
                            .await;
                        continue;
                    }
                };
                let (stream_tx, stream_rx) = mpsc::channel::<Vec<u8>>(64);
                streams.insert(frame.stream_id, stream_tx);
                let tx = tx.clone();
                let connect_timeout = Duration::from_secs(config.connect_timeout_secs);
                let allow_private = config.allow_private_targets;
                tokio::spawn(async move {
                    handle_stream(
                        frame.stream_id,
                        target,
                        tx,
                        stream_rx,
                        connect_timeout,
                        allow_private,
                    )
                    .await;
                });
            }
            frame_type::OPEN_UDP => {
                let (udp_tx, udp_rx) = mpsc::channel::<(TargetAddr, Vec<u8>)>(64);
                udp_streams.insert(frame.stream_id, udp_tx);
                let tx = tx.clone();
                let connect_timeout = Duration::from_secs(config.connect_timeout_secs);
                let allow_private = config.allow_private_targets;
                tokio::spawn(async move {
                    handle_udp_stream(
                        frame.stream_id,
                        tx,
                        udp_rx,
                        connect_timeout,
                        allow_private,
                    )
                    .await;
                });
            }
            frame_type::UDP_DATA => {
                if let Some(stream) = udp_streams.get(&frame.stream_id) {
                    match frame.decode_udp_data() {
                        Ok((target, data)) => {
                            let _ = stream.send((target, data.to_vec())).await;
                        }
                        Err(error) => {
                            debug!(error = %error, "bad UDP frame");
                        }
                    }
                }
            }
            frame_type::DATA => {
                if let Some(stream) = streams.get(&frame.stream_id) {
                    if let Ok(data) = frame.decode_data() {
                        let _ = stream.send(data.to_vec()).await;
                    }
                }
            }
            frame_type::CLOSE => {
                streams.remove(&frame.stream_id);
                udp_streams.remove(&frame.stream_id);
            }
            frame_type::PING => {
                let _ = tx
                    .send(Frame::pong(frame.stream_id, frame.body.clone()))
                    .await;
            }
            _ => {}
        }
    }

    drop(streams);
    drop(tx);
    let _ = writer_task.await;
    Ok(())
}

async fn handle_udp_stream(
    stream_id: u32,
    tx: mpsc::Sender<Frame>,
    mut rx: mpsc::Receiver<(TargetAddr, Vec<u8>)>,
    _connect_timeout: Duration,
    allow_private: bool,
) {
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(socket) => Arc::new(socket),
        Err(error) => {
            warn!(stream_id, error = %error, "UDP bind failed");
            let _ = tx
                .send(Frame::open_result(stream_id, open_status::CONNECT))
                .await;
            return;
        }
    };
    if tx
        .send(Frame::open_result(stream_id, open_status::OK))
        .await
        .is_err()
    {
        return;
    }

    let mut buffer = vec![0u8; 65_535];
    loop {
        tokio::select! {
            incoming = rx.recv() => {
                let Some((target, data)) = incoming else {
                    break;
                };
                let resolved = match resolve_target(&target, allow_private).await {
                    Ok(addr) => addr,
                    Err(status) => {
                        debug!(stream_id, target = ?target, status, "UDP target rejected");
                        continue;
                    }
                };
                if let Err(error) = socket.send_to(&data, resolved).await {
                    debug!(stream_id, error = %error, "UDP send failed");
                }
            }
            received = socket.recv_from(&mut buffer) => {
                let Ok((len, source)) = received else {
                    break;
                };
                let frame = match Frame::udp_data(
                    stream_id,
                    &TargetAddr::Ip(source),
                    &buffer[..len],
                ) {
                    Ok(frame) => frame,
                    Err(_) => continue,
                };
                if tx.send(frame).await.is_err() {
                    break;
                }
            }
        }
    }
}

async fn resolve_target(target: &TargetAddr, allow_private: bool) -> Result<SocketAddr, u8> {
    match target {
        TargetAddr::Ip(addr) => {
            if allow_private || !is_forbidden(addr.ip()) {
                Ok(*addr)
            } else {
                Err(open_status::POLICY)
            }
        }
        TargetAddr::Domain(host, port) => {
            let mut candidates = lookup_host((host.as_str(), *port))
                .await
                .map_err(|_| open_status::DNS)?;
            candidates
                .find(|addr| allow_private || !is_forbidden(addr.ip()))
                .ok_or(open_status::POLICY)
        }
    }
}

/// Reject loopback, private, link-local, CGNAT, benchmark and reserved ranges.
/// Without this an exposed pvpn-server would be an SSRF proxy into its own LAN.
fn is_forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ipv4_forbidden(ip),
        IpAddr::V6(ip) => ipv6_forbidden(ip),
    }
}

fn ipv4_forbidden(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || o[0] == 0
        || o[0] >= 240
        || (o[0] == 100 && (64..=127).contains(&o[1]))
        || (o[0] == 198 && (18..=19).contains(&o[1]))
}

fn ipv6_forbidden(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
}

async fn handle_stream(
    stream_id: u32,
    target: TargetAddr,
    tx: mpsc::Sender<Frame>,
    mut rx: mpsc::Receiver<Vec<u8>>,
    connect_timeout: Duration,
    allow_private: bool,
) {
    debug!(stream_id, target = ?target, "opening target");

    let resolved = match resolve_target(&target, allow_private).await {
        Ok(addr) => addr,
        Err(status) => {
            let _ = tx.send(Frame::open_result(stream_id, status)).await;
            return;
        }
    };

    let socket = match timeout(connect_timeout, TcpStream::connect(resolved)).await {
        Ok(Ok(socket)) => socket,
        Ok(Err(error)) => {
            warn!(stream_id, error = %error, "target connect failed");
            let _ = tx
                .send(Frame::open_result(stream_id, open_status::CONNECT))
                .await;
            return;
        }
        Err(_) => {
            let _ = tx
                .send(Frame::open_result(stream_id, open_status::TIMEOUT))
                .await;
            return;
        }
    };

    let _ = socket.set_nodelay(true);
    if tx
        .send(Frame::open_result(stream_id, open_status::OK))
        .await
        .is_err()
    {
        return;
    }

    let (mut target_reader, mut target_writer) = socket.into_split();
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            result = target_reader.read(&mut buffer) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        let frame = Frame::data_padded(stream_id, buffer[..n].to_vec());
                        if tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            data = rx.recv() => {
                match data {
                    Some(data) => {
                        if target_writer.write_all(&data).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    let _ = tx.send(Frame::close(stream_id, 0)).await;
}
