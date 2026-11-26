use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::{ChaCha20, Key as ChaChaKey, Nonce as ChaChaNonce};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::config::Psk;
use crate::frame::{Frame, FrameError};

pub const DIR_C2S: u32 = 1;
pub const DIR_S2C: u32 = 2;

const AUTH_SALT: &[u8] = b"pvpn-auth-v1";
const AUTH_INFO: &[u8] = b"handshake";
const SESSION_INFO: &[u8] = b"pvpn-session-v1";
const MAX_FRAME: usize = 65_535;

/// 32-byte key used only for handshake authentication.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct AuthKey([u8; 32]);

impl AuthKey {
    pub fn derive(psk: &Psk) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(AUTH_SALT), psk.as_bytes());
        let mut out = [0u8; 32];
        hk.expand(AUTH_INFO, &mut out)
            .expect("32 bytes is a valid HKDF output length");
        Self(out)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Per-direction frame keys and the cosmetic XOR mask key.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct SessionKeys {
    pub c2s: [u8; 32],
    pub s2c: [u8; 32],
    pub mask: [u8; 32],
}

impl SessionKeys {
    pub fn derive(shared: &[u8; 32], client_nonce: &[u8; 16], server_nonce: &[u8; 16]) -> Self {
        let mut salt = [0u8; 32];
        salt[..16].copy_from_slice(client_nonce);
        salt[16..].copy_from_slice(server_nonce);
        let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
        let mut okm = [0u8; 96];
        hk.expand(SESSION_INFO, &mut okm)
            .expect("96 bytes is a valid HKDF output length");
        let mut keys = Self {
            c2s: [0u8; 32],
            s2c: [0u8; 32],
            mask: [0u8; 32],
        };
        keys.c2s.copy_from_slice(&okm[..32]);
        keys.s2c.copy_from_slice(&okm[32..64]);
        keys.mask.copy_from_slice(&okm[64..96]);
        okm.zeroize();
        keys
    }

    /// Returns `(send, recv)` for a client.
    pub fn client_ciphers(&self) -> (FrameCipher, FrameCipher) {
        (
            FrameCipher::new(self.c2s, self.mask, DIR_C2S),
            FrameCipher::new(self.s2c, self.mask, DIR_S2C),
        )
    }

    /// Returns `(send, recv)` for a server.
    pub fn server_ciphers(&self) -> (FrameCipher, FrameCipher) {
        (
            FrameCipher::new(self.s2c, self.mask, DIR_S2C),
            FrameCipher::new(self.c2s, self.mask, DIR_C2S),
        )
    }
}

pub fn hmac_tag(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

pub fn verify_hmac(key: &[u8; 32], parts: &[&[u8]], tag: &[u8; 32]) -> bool {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    mac.verify_slice(tag).is_ok()
}

/// Stateful frame sealer/opener for one direction.
pub struct FrameCipher {
    aead: ChaCha20Poly1305,
    mask_key: [u8; 32],
    len_mask: u32,
    dir: u32,
    seq: u64,
}

impl FrameCipher {
    pub fn new(key: [u8; 32], mask_key: [u8; 32], dir: u32) -> Self {
        let aead = ChaCha20Poly1305::new(Key::from_slice(&key));
        let len_mask = u32::from_be_bytes([mask_key[0], mask_key[1], mask_key[2], mask_key[3]]);
        Self {
            aead,
            mask_key,
            len_mask,
            dir,
            seq: 0,
        }
    }

    pub fn sequence(&self) -> u64 {
        self.seq
    }

    /// Decode the masked length from a 4-byte frame header.
    pub fn encrypted_len(&self, header: &[u8; 4]) -> Result<usize, FrameError> {
        let masked_len = u32::from_be_bytes(*header);
        let len = (masked_len ^ self.len_mask) as usize;
        if !(16..=MAX_FRAME).contains(&len) {
            return Err(FrameError::BadLength(len));
        }
        Ok(len)
    }

    fn nonce(&self, seq: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.dir.to_be_bytes());
        nonce[4..].copy_from_slice(&seq.to_be_bytes());
        nonce
    }

    pub fn seal(&mut self, frame: &Frame) -> Result<Vec<u8>, FrameError> {
        let plain = frame.encode();
        let ciphertext_len = plain.len() + 16;
        if ciphertext_len > MAX_FRAME {
            return Err(FrameError::TooLarge(ciphertext_len));
        }
        let masked_len = (ciphertext_len as u32) ^ self.len_mask;
        let aad = masked_len.to_be_bytes();
        let nonce = self.nonce(self.seq);
        let ciphertext = self
            .aead
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: &aad,
                },
            )
            .map_err(|_| FrameError::Crypto)?;

        let mut masked = ciphertext;
        self.apply_mask(&nonce, &mut masked);

        let mut wire = Vec::with_capacity(4 + masked.len());
        wire.extend_from_slice(&aad);
        wire.extend_from_slice(&masked);
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or(FrameError::SequenceExhausted)?;
        Ok(wire)
    }

    pub fn open(&mut self, wire: &[u8]) -> Result<Frame, FrameError> {
        if wire.len() < 4 {
            return Err(FrameError::Truncated);
        }
        let masked_len = u32::from_be_bytes([wire[0], wire[1], wire[2], wire[3]]);
        let len = (masked_len ^ self.len_mask) as usize;
        if !(16..=MAX_FRAME).contains(&len) {
            return Err(FrameError::BadLength(len));
        }
        if wire.len() != 4 + len {
            return Err(FrameError::BadLength(wire.len().saturating_sub(4)));
        }
        let mut ciphertext = wire[4..].to_vec();
        let nonce = self.nonce(self.seq);
        self.apply_mask(&nonce, &mut ciphertext);
        let aad = masked_len.to_be_bytes();
        let plain = self
            .aead
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| FrameError::Crypto)?;
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or(FrameError::SequenceExhausted)?;
        Frame::decode(&plain)
    }

    fn apply_mask(&self, nonce: &[u8; 12], bytes: &mut [u8]) {
        let mut cipher = ChaCha20::new(
            ChaChaKey::from_slice(&self.mask_key),
            ChaChaNonce::from_slice(nonce),
        );
        cipher.apply_keystream(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{frame_type, Frame};

    fn keys() -> SessionKeys {
        SessionKeys {
            c2s: [7u8; 32],
            s2c: [9u8; 32],
            mask: [11u8; 32],
        }
    }

    #[test]
    fn frame_roundtrip_and_mask() {
        let k = keys();
        let (mut send, _) = k.client_ciphers();
        let (_, mut recv) = k.server_ciphers();
        let frame = Frame::data(1, b"hello world".to_vec());
        let wire = send.seal(&frame).unwrap();
        assert_ne!(&wire[4..], b"hello world");
        let got = recv.open(&wire).unwrap();
        assert_eq!(got.typ, frame_type::DATA);
        assert_eq!(got.stream_id, 1);
        assert_eq!(got.decode_data().unwrap(), b"hello world");
    }

    #[test]
    fn tampered_frame_is_rejected() {
        let k = keys();
        let (mut send, _) = k.client_ciphers();
        let (_, mut recv) = k.server_ciphers();
        let mut wire = send.seal(&Frame::ping(1)).unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 0x80;
        assert!(recv.open(&wire).is_err());
    }
}
