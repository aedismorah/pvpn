use rand::rngs::OsRng;
use rand::RngCore;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use crate::config::Psk;
use crate::crypto::{hmac_tag, verify_hmac, AuthKey, FrameCipher, SessionKeys};

const VERSION: u8 = 2;
const HELLO_LEN: usize = 128;
const CLIENT_FIXED: usize = 1 + 32 + 16 + 32; // 81
const SERVER_FIXED: usize = 32 + 16 + 32; // 80

type HelloParts = ([u8; 32], [u8; 16], [u8; 32]);

#[derive(Debug, thiserror::Error)]
pub enum HandshakeError {
    #[error("handshake authentication failed")]
    Auth,
    #[error("unsupported protocol version {0}")]
    Version(u8),
    #[error("bad hello padding")]
    Padding,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub async fn client_handshake<S>(
    stream: &mut S,
    psk: &Psk,
) -> Result<(FrameCipher, FrameCipher), HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let auth = AuthKey::derive(psk);
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let mut client_nonce = [0u8; 16];
    OsRng.fill_bytes(&mut client_nonce);

    let tag = hmac_tag(
        auth.as_bytes(),
        &[b"C", &[VERSION], public.as_bytes(), &client_nonce],
    );
    let hello = build_client_hello(public.as_bytes(), &client_nonce, &tag);
    stream.write_all(&hello).await?;
    stream.flush().await?;

    let mut response = [0u8; HELLO_LEN];
    stream.read_exact(&mut response).await?;
    let (server_public, server_nonce, server_tag) = parse_server_hello(&response)?;
    if !verify_hmac(
        auth.as_bytes(),
        &[
            b"S",
            &[VERSION],
            public.as_bytes(),
            &server_public,
            &client_nonce,
            &server_nonce,
        ],
        &server_tag,
    ) {
        return Err(HandshakeError::Auth);
    }

    let shared = secret.diffie_hellman(&PublicKey::from(server_public));
    let keys = SessionKeys::derive(shared.as_bytes(), &client_nonce, &server_nonce);
    Ok(keys.client_ciphers())
}

pub async fn server_handshake<S>(
    stream: &mut S,
    psk: &Psk,
) -> Result<(FrameCipher, FrameCipher), HandshakeError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let auth = AuthKey::derive(psk);
    let mut hello = [0u8; HELLO_LEN];
    stream.read_exact(&mut hello).await?;
    let parsed = parse_client_hello(&hello);
    let authenticated = match &parsed {
        Ok((client_public, client_nonce, client_tag)) => verify_hmac(
            auth.as_bytes(),
            &[b"C", &[VERSION], client_public, client_nonce],
            client_tag,
        ),
        Err(_) => false,
    };
    if !authenticated {
        // Send an indistinguishable random response before closing so an
        // active prober cannot tell a bad PSK from a malformed hello.
        let mut noise = [0u8; HELLO_LEN];
        OsRng.fill_bytes(&mut noise);
        stream.write_all(&noise).await?;
        stream.flush().await?;
        return Err(HandshakeError::Auth);
    }
    let (client_public, client_nonce, _) = parsed.map_err(|_| HandshakeError::Auth)?;

    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let mut server_nonce = [0u8; 16];
    OsRng.fill_bytes(&mut server_nonce);

    let tag = hmac_tag(
        auth.as_bytes(),
        &[
            b"S",
            &[VERSION],
            &client_public,
            public.as_bytes(),
            &client_nonce,
            &server_nonce,
        ],
    );
    let response = build_server_hello(public.as_bytes(), &server_nonce, &tag);
    stream.write_all(&response).await?;
    stream.flush().await?;

    let shared = secret.diffie_hellman(&PublicKey::from(client_public));
    let keys = SessionKeys::derive(shared.as_bytes(), &client_nonce, &server_nonce);
    Ok(keys.server_ciphers())
}

fn build_client_hello(public: &[u8; 32], nonce: &[u8; 16], tag: &[u8; 32]) -> [u8; HELLO_LEN] {
    let mut msg = [0u8; HELLO_LEN];
    OsRng.fill_bytes(&mut msg);
    msg[0] = VERSION;
    msg[1..33].copy_from_slice(public);
    msg[33..49].copy_from_slice(nonce);
    msg[49..81].copy_from_slice(tag);
    let pad_len = (HELLO_LEN - CLIENT_FIXED - 2) as u16;
    msg[81..83].copy_from_slice(&pad_len.to_be_bytes());
    msg
}

fn build_server_hello(public: &[u8; 32], nonce: &[u8; 16], tag: &[u8; 32]) -> [u8; HELLO_LEN] {
    let mut msg = [0u8; HELLO_LEN];
    OsRng.fill_bytes(&mut msg);
    msg[..32].copy_from_slice(public);
    msg[32..48].copy_from_slice(nonce);
    msg[48..80].copy_from_slice(tag);
    let pad_len = (HELLO_LEN - SERVER_FIXED - 2) as u16;
    msg[80..82].copy_from_slice(&pad_len.to_be_bytes());
    msg
}

fn parse_client_hello(msg: &[u8; HELLO_LEN]) -> Result<HelloParts, HandshakeError> {
    if msg[0] != VERSION {
        return Err(HandshakeError::Version(msg[0]));
    }
    let mut public = [0u8; 32];
    public.copy_from_slice(&msg[1..33]);
    let mut nonce = [0u8; 16];
    nonce.copy_from_slice(&msg[33..49]);
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&msg[49..81]);
    let pad_len = u16::from_be_bytes([msg[81], msg[82]]) as usize;
    if CLIENT_FIXED + 2 + pad_len != HELLO_LEN {
        return Err(HandshakeError::Padding);
    }
    Ok((public, nonce, tag))
}

fn parse_server_hello(msg: &[u8; HELLO_LEN]) -> Result<HelloParts, HandshakeError> {
    let mut public = [0u8; 32];
    public.copy_from_slice(&msg[..32]);
    let mut nonce = [0u8; 16];
    nonce.copy_from_slice(&msg[32..48]);
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&msg[48..80]);
    let pad_len = u16::from_be_bytes([msg[80], msg[81]]) as usize;
    if SERVER_FIXED + 2 + pad_len != HELLO_LEN {
        return Err(HandshakeError::Padding);
    }
    Ok((public, nonce, tag))
}

/// Wipe a session key buffer after use (helper for callers that keep raw keys).
pub fn wipe(bytes: &mut [u8]) {
    bytes.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Psk;

    #[tokio::test]
    async fn handshake_derives_matching_ciphers() {
        let psk = Psk::generate();
        let (mut client, mut server) = tokio::io::duplex(512);
        let psk_server = psk.clone();
        let server_task =
            tokio::spawn(async move { server_handshake(&mut server, &psk_server).await.unwrap() });
        let (mut send, _) = client_handshake(&mut client, &psk).await.unwrap();
        let (_, mut recv) = server_task.await.unwrap();
        let frame = crate::frame::Frame::ping(1);
        let wire = send.seal(&frame).unwrap();
        let got = recv.open(&wire).unwrap();
        assert_eq!(got.typ, crate::frame::frame_type::PING);
    }

    #[tokio::test]
    async fn wrong_psk_is_rejected() {
        let psk = Psk::generate();
        let wrong = Psk::generate();
        let (mut client, mut server) = tokio::io::duplex(512);
        let server_task = tokio::spawn(async move { server_handshake(&mut server, &wrong).await });
        let client_result = client_handshake(&mut client, &psk).await;
        assert!(client_result.is_err() || server_task.await.unwrap().is_err());
    }
}
