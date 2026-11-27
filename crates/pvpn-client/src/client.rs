use std::sync::Arc;
use std::time::Duration;

use pvpn_core::frame::{frame_type, open_status};
use pvpn_core::{
    client_handshake, read_frame, write_frame, ClientConfig, Frame, FrameCipher, TargetAddr,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

macro_rules! udp_debug {
    ($($arg:tt)*) => { () };
}
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tracing::{debug, info};

use crate::socks5;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_ID: u32 = 1;

pub async fn run(config: ClientConfig) -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let result = run_until(config, shutdown_rx).await;
    drop(shutdown_tx);
    result
}

pub async fn run_until(
    config: ClientConfig,
    shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let socks_listen = config.listen.clone();
    let http_listen = config.http_listen.clone();
    run_with_listeners(config, socks_listen, http_listen, shutdown).await
}

/// Run SOCKS5 and optional HTTP proxy listeners over one shared tunnel config.
pub async fn run_with_listeners(
    config: ClientConfig,
    socks_listen: String,
    http_listen: Option<String>,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let socks = TcpListener::bind(&socks_listen).await?;
    let http = match http_listen.as_deref() {
        Some(listen) if !listen.is_empty() => Some(TcpListener::bind(listen).await?),
        _ => None,
    };
    info!(
        socks = %socks_listen,
        http = ?http_listen,
        server = %config.server,
        "pvpn-client listening"
    );

    let config = Arc::new(config);
    let socks_task = tokio::spawn(accept_socks(socks, config.clone()));
    let http_task = http.map(|listener| tokio::spawn(accept_http(listener, config)));

    let _ = shutdown.changed().await;
    socks_task.abort();
    if let Some(task) = http_task {
        task.abort();
    }
    Ok(())
}

async fn accept_socks(listener: TcpListener, config: Arc<ClientConfig>) {
    loop {
        match listener.accept().await {
            Ok((socket, peer)) => {
                let config = config.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_socks(socket, config).await {
                        debug!(%peer, error = %error, "socks connection closed");
                    }
                });
            }
            Err(error) => {
                debug!(error = %error, "socks accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn accept_http(listener: TcpListener, config: Arc<ClientConfig>) {
    loop {
        match listener.accept().await {
            Ok((socket, peer)) => {
                let config = config.clone();
                tokio::spawn(async move {
                    if let Err(error) = crate::http_proxy::handle(socket, config).await {
                        debug!(%peer, error = %error, "http proxy connection closed");
                    }
                });
            }
            Err(error) => {
                debug!(error = %error, "http accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

async fn handle_socks(mut local: TcpStream, config: Arc<ClientConfig>) -> anyhow::Result<()> {
    local.set_nodelay(true)?;
    let request = socks5::handshake(&mut local).await?;
    match request {
        socks5::Request::Connect(target) => {
            debug!(target = ?target, "socks request");
            match open_session(&config, &target).await {
                Ok(session) => {
                    socks5::reply_ok(&mut local).await?;
                    relay(&mut local, session).await?;
                }
                Err(error) => {
                    debug!(error = %error, "session open failed");
                    socks5::reply_failure(&mut local).await?;
                }
            }
        }
        socks5::Request::UdpAssociate => {
            udp_debug!("udp associate requested");
            let udp = UdpSocket::bind("127.0.0.1:0").await?;
            let port = udp.local_addr()?.port();
            udp_debug!(&format!("udp bound port {port}"));
            let session = match open_udp_session(&config).await {
                Ok(session) => session,
                Err(error) => {
                    udp_debug!(&format!("udp control failed: {error}"));
                    debug!(error = %error, "UDP session open failed");
                    socks5::reply_failure(&mut local).await?;
                    return Ok(());
                }
            };
            udp_debug!("udp control open");
            socks5::reply_udp(&mut local, port).await?;
            udp_debug!("udp reply sent");
            relay_udp(&mut local, udp, session).await?;
        }
    }
    Ok(())
}

pub(crate) struct Session {
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    send: FrameCipher,
    recv: FrameCipher,
    max_frame: usize,
}

pub(crate) async fn open_session(
    config: &ClientConfig,
    target: &TargetAddr,
) -> anyhow::Result<Session> {
    open_control(config, Frame::open_tcp(STREAM_ID, target)?).await
}

async fn open_udp_session(config: &ClientConfig) -> anyhow::Result<Session> {
    udp_debug!(&format!("udp control connecting to {}", config.server));
    let result = open_control(config, Frame::open_udp(STREAM_ID)).await;
    match &result {
        Ok(_) => udp_debug!("udp control connected"),
        Err(_error) => udp_debug!(&format!("udp control _error: {_error}")),
    }
    result
}

async fn open_control(config: &ClientConfig, open: Frame) -> anyhow::Result<Session> {
    let is_udp = open.typ == frame_type::OPEN_UDP;
    if is_udp {
        udp_debug!("control tcp connecting");
    }
    let connect_timeout = Duration::from_secs(config.connect_timeout_secs.max(1));
    let mut socket = timeout(connect_timeout, TcpStream::connect(&config.server))
        .await
        .map_err(|_| anyhow::anyhow!("connect timeout"))??;
    if is_udp {
        udp_debug!("control tcp connected");
    }
    socket.set_nodelay(true)?;
    let (send, recv) = timeout(
        HANDSHAKE_TIMEOUT,
        client_handshake(&mut socket, &config.psk),
    )
    .await
    .map_err(|_| anyhow::anyhow!("handshake timeout"))??;
    if is_udp {
        udp_debug!("control handshake done");
    }

    let (mut reader, mut writer) = socket.into_split();
    let mut send = send;
    let mut recv = recv;
    write_frame(&mut writer, &mut send, &open).await?;
    if is_udp {
        udp_debug!("control open frame sent");
    }

    loop {
        let frame = timeout(
            connect_timeout,
            read_frame(&mut reader, &mut recv, 65_535),
        )
        .await
        .map_err(|_| anyhow::anyhow!("OPEN_RESULT timeout"))??;
        if is_udp {
            udp_debug!(&format!("control frame type {:#x}", frame.typ));
        }
        match frame.typ {
            frame_type::OPEN_RESULT => {
                let status = frame.body.first().copied().unwrap_or(255);
                if status == open_status::OK {
                    return Ok(Session {
                        reader,
                        writer,
                        send,
                        recv,
                        max_frame: 65_535,
                    });
                }
                anyhow::bail!("server rejected target with status {status}");
            }
            frame_type::ERROR => {
                let code = frame
                    .body
                    .get(..2)
                    .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                    .unwrap_or(0);
                anyhow::bail!("server error {code}");
            }
            frame_type::PADDING => continue,
            frame_type::PING => {
                write_frame(
                    &mut writer,
                    &mut send,
                    &Frame::pong(frame.stream_id, frame.body.clone()),
                )
                .await?;
            }
            _ => continue,
        }
    }
}

pub(crate) async fn relay_udp(
    local: &mut TcpStream,
    udp: UdpSocket,
    session: Session,
) -> anyhow::Result<()> {
    let Session {
        reader,
        mut writer,
        mut send,
        recv,
        max_frame,
    } = session;
    let (frame_tx, mut frame_rx) = mpsc::channel(32);
    let reader_task = tokio::spawn(frame_pump(reader, recv, max_frame, frame_tx));
    let mut buffer = vec![0u8; 65_535];
    let mut control = [0u8; 1];
    let mut client = None;
    udp_debug!("relay udp start");
    loop {
        tokio::select! {
            control_read = local.read(&mut control) => {
                if control_read.unwrap_or(0) == 0 {
                    udp_debug!("udp control closed");
                    break;
                }
            }
            received = udp.recv_from(&mut buffer) => {
                let Ok((len, source)) = received else {
                    udp_debug!("udp recv error");
                    break;
                };
                client = Some(source);
                let Some((target, payload)) = socks5::parse_udp(&buffer[..len]) else {
                    continue;
                };
                let frame = match Frame::udp_data(STREAM_ID, &target, payload) {
                    Ok(frame) => frame,
                    Err(_) => continue,
                };
                if write_frame(&mut writer, &mut send, &frame).await.is_err() {
                    break;
                }
            }
            incoming = frame_rx.recv() => {
                let Some(frame) = incoming else {
                    break;
                };
                match frame.typ {
                    frame_type::UDP_DATA => {
                        if let Ok((target, payload)) = frame.decode_udp_data() {
                            if let Some(client) = client {
                                let packet = socks5::encode_udp(&target, payload);
                                let _ = udp.send_to(&packet, client).await;
                            }
                        }
                    }
                    frame_type::CLOSE | frame_type::ERROR => break,
                    frame_type::PING => {
                        write_frame(
                            &mut writer,
                            &mut send,
                            &Frame::pong(frame.stream_id, frame.body.clone()),
                        ).await?;
                    }
                    _ => {}
                }
            }
        }
    }
    reader_task.abort();
    let _ = write_frame(&mut writer, &mut send, &Frame::close(STREAM_ID, 0)).await;
    Ok(())
}

pub(crate) async fn relay(local: &mut TcpStream, session: Session) -> anyhow::Result<()> {
    relay_with_initial(local, session, &[]).await
}

pub(crate) async fn relay_with_initial(
    local: &mut TcpStream,
    session: Session,
    initial: &[u8],
) -> anyhow::Result<()> {
    let Session {
        reader,
        mut writer,
        mut send,
        recv,
        max_frame,
    } = session;
    if !initial.is_empty() {
        let frame = Frame::data_padded(STREAM_ID, initial.to_vec());
        write_frame(&mut writer, &mut send, &frame).await?;
    }

    let (frame_tx, mut frame_rx) = mpsc::channel(32);
    let reader_task = tokio::spawn(frame_pump(reader, recv, max_frame, frame_tx));
    let (mut local_reader, mut local_writer) = local.split();
    let mut buffer = vec![0u8; 16 * 1024];
    let mut local_open = true;

    loop {
        tokio::select! {
            result = local_reader.read(&mut buffer), if local_open => {
                match result {
                    Ok(0) => {
                        // Half-close: tell the exit to FIN the target write side,
                        // but keep reading until the target closes its own side.
                        let _ = write_frame(
                            &mut writer,
                            &mut send,
                            &Frame::shutdown_write(STREAM_ID),
                        ).await;
                        local_open = false;
                    }
                    Ok(n) => {
                        let frame = Frame::data_padded(STREAM_ID, buffer[..n].to_vec());
                        if write_frame(&mut writer, &mut send, &frame).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            incoming = frame_rx.recv() => {
                let Some(frame) = incoming else {
                    break;
                };
                match frame.typ {
                    frame_type::DATA => {
                        let data = frame.decode_data()?;
                        if local_writer.write_all(data).await.is_err() {
                            break;
                        }
                    }
                    frame_type::CLOSE | frame_type::ERROR => break,
                    frame_type::PING => {
                        write_frame(
                            &mut writer,
                            &mut send,
                            &Frame::pong(frame.stream_id, frame.body.clone()),
                        ).await?;
                    }
                    _ => {}
                }
            }
        }
    }
    reader_task.abort();
    let _ = write_frame(&mut writer, &mut send, &Frame::close(STREAM_ID, 0)).await;
    Ok(())
}

async fn frame_pump(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    mut recv: FrameCipher,
    max_frame: usize,
    tx: mpsc::Sender<Frame>,
) {
    loop {
        match read_frame(&mut reader, &mut recv, max_frame).await {
            Ok(frame) => {
                if tx.send(frame).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
