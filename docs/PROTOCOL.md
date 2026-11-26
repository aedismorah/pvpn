# Wire protocol v1

All integers are big-endian unless stated otherwise. `||` is concatenation.

## Primitives

- X25519 for ephemeral key agreement.
- HKDF-SHA256 for key derivation.
- HMAC-SHA256 for handshake authentication.
- ChaCha20-Poly1305 for frames.
- ChaCha20 for the XOR mask keystream (cosmetic layer).
- 32-byte pre-shared key (PSK), configured out of band.

The PSK is **not** sent on the wire. It authenticates the handshake and keys the
initial derivation. Ephemeral X25519 provides forward secrecy for the session.

## Handshake

### ClientHello (padded to 128 bytes)

| Field | Size |
| --- | --- |
| version | 1 |
| client_ephemeral_public | 32 |
| client_nonce | 16 |
| client_auth_tag | 32 |
| pad_len | 2 |
| random padding | 0..=45 |

```text
auth_key = HKDF-SHA256(psk, salt="pvpn-auth-v1", info="handshake", 32)
client_auth_tag = HMAC-SHA256(auth_key,
    "C" || version || client_ephemeral_public || client_nonce)
```

### ServerHello (padded to 128 bytes)

| Field | Size |
| --- | --- |
| server_ephemeral_public | 32 |
| server_nonce | 16 |
| server_auth_tag | 32 |
| pad_len | 2 |
| random padding | 0..=45 |

```text
server_auth_tag = HMAC-SHA256(auth_key,
    "S" || version || client_ephemeral_public || server_ephemeral_public ||
    client_nonce || server_nonce)
```

Both sides compute:

```text
shared = X25519(own_ephemeral_secret, peer_ephemeral_public)
salt   = client_nonce || server_nonce
prk    = HKDF-Extract(salt, shared)
okm    = HKDF-Expand(prk, "pvpn-session-v1", 96)
c2s_key  = okm[0..32]
s2c_key  = okm[32..64]
mask_key = okm[64..96]
```

The server rejects a bad tag with a generic close. The client aborts on a bad
server tag.

## Frame format

```text
+----------------+-----------------------------+
| masked_len: u32| masked_ciphertext: N bytes  |
+----------------+-----------------------------+
```

- `plaintext = type:u8 || stream_id:u32 || body`
- `ciphertext = AEAD_encrypt(key, nonce, plaintext, aad=masked_len)`
- `masked_len = len(ciphertext) XOR len_mask`
- `masked_ciphertext = ciphertext XOR ChaCha20(mask_key, nonce)[0..N]`
- `len_mask` = first 4 bytes of the shared `mask_key` interpreted big-endian.
- AEAD nonce = `dir:u32 || seq:u64`, where `dir=1` for client->server and
  `dir=2` for server->client; `seq` starts at 0 and increments per frame.
- Max ciphertext length: 65535.

The receiver reads `masked_len`, unmasks it, reads exactly that many bytes,
unmasks the ciphertext, decrypts with `aad=masked_len`, and checks `seq`.

The XOR mask is not required for confidentiality (AEAD provides it). It exists
to make the wire bytes differ from a standard AEAD record and is documented as a
cosmetic layer.

## Frame types

| Type | Name | Body |
| --- | --- | --- |
| 0x01 | OPEN_TCP | `addr_type:u8, addr_len:u8, addr, port:u16` |
| 0x02 | DATA | `data_len:u16, data, padding` |
| 0x03 | CLOSE | `reason:u8` |
| 0x04 | PING | random 8 bytes |
| 0x05 | PONG | copy of ping |
| 0x06 | PADDING | random bytes |
| 0x07 | OPEN_RESULT | `status:u8` |
| 0x08 | ERROR | `code:u16, utf8 message` |
| 0x09 | OPEN_UDP | empty (server binds a UDP socket) |
| 0x0A | UDP_DATA | `target address, payload` |

Status codes for OPEN_RESULT: `0` ok, `1` DNS failure, `2` connect failure,
`3` timeout, `4` policy rejection.

Address types: `1` IPv4, `2` domain, `3` IPv6.

## Stream lifecycle

```text
client                                      server
  | OPEN_TCP(stream_id=1, target)  ---------> |
  | <--------------------- OPEN_RESULT(ok)    |
  | DATA(stream_id=1, bytes)       ---------> | --> target socket
  | <--------------------- DATA(stream_id=1)  | <-- target socket
  | CLOSE(stream_id=1)             ---------> |
```

The first client implementation opens one session per SOCKS connection and uses
`stream_id=1`. The server supports multiple stream IDs for future multiplexing.

## UDP lifecycle

SOCKS5 UDP ASSOCIATE is supported. The client binds a local UDP socket, replies
with its `127.0.0.1:port`, opens a pvpn session with `OPEN_UDP`, and carries each
SOCKS5 UDP datagram in `UDP_DATA`. The server resolves the target, sends the
datagram from a fresh UDP socket, and returns replies as `UDP_DATA` frames with
the responder address. This carries QUIC/HTTP3, DNS-over-UDP and other datagrams;
it is required for YouTube and for Telegram calls.

## Limits

- handshake timeout: 10 s
- connect timeout: 15 s
- idle timeout: 10 min (planned)
- max frame: 64 KiB
- max concurrent streams per session: 64 (planned enforcement)
