# Architecture

## Goals

- Work on Windows and Android.
- Use a Russian ingress host (Cloud.ru) and a Netherlands exit host (VDSina).
- Do not look like VLESS/WireGuard/OpenVPN/SOCKS on the wire.
- Use non-standard TCP ports.
- Keep the cloud ingress blind to payloads.
- Prefer Rust for the data plane and protocol.

## Data path

```text
+------------------+        TCP/443         +---------------------+
| Windows / Android|  opaque framed stream   | Cloud.ru            |
| pvpn-client      | <---------------------> | TCP forwarder       |
+------------------+                         +----------+----------+
                                                        | TCP/18443
                                                        v
                                             +---------------------+
                                             | VDSina NL           |
                                             | pvpn-server         |
                                             | decrypt + dial out  |
                                             +----------+----------+
                                                        |
                                                        v
                                                    Internet
```

The Cloud.ru host does **not** terminate the protocol. It forwards TCP bytes to
VDSina. Therefore the client's cryptographic session ends at VDSina, and the
Russian host cannot read destinations or payloads.

For the first prototype the forwarder can be `socat`/`nftables`/HAProxy. Later a
small Rust forwarder with connection limits and replay protection can replace it.

## Components

### `pvpn-core`

Platform-independent library:

- `config`: TOML configuration structs.
- `crypto`: PSK auth, X25519 ephemeral exchange, HKDF key schedule,
  ChaCha20-Poly1305 frame encryption, XOR wire mask.
- `handshake`: random-looking fixed-size hello messages.
- `frame`: length-framed, masked, AEAD-protected frames; TCP stream frames.

### `pvpn-server`

Runs on VDSina (Linux, later Windows for tests):

- listens on a non-standard port;
- authenticates clients with PSK + ephemeral X25519;
- demultiplexes streams by `stream_id`;
- dials TCP destinations with timeouts and size limits;
- returns `OPEN_RESULT`, `DATA`, `CLOSE`.

### `pvpn-client`

Runs on Windows (later Android native core):

- exposes a local SOCKS5 proxy on loopback;
- for each SOCKS CONNECT, opens one protocol session to the server;
- relays bytes in `DATA` frames;
- supports domain, IPv4 and IPv6 targets.

A Tauri/WinUI shell can be added later; the CLI is the reference client.

### Android

Planned: Kotlin `VpnService` + Rust `cdylib` via JNI. A full user-space TCP/IP
stack is required (smoltcp or an existing tun2socks core). This is a separate
milestone; the protocol library is `no_std`-friendly but currently uses `std`.

## Ports

| Link | Port | Why |
| --- | --- | --- |
| client -> Cloud.ru | 443 | non-standard, not a well-known VPN port |
| Cloud.ru -> VDSina | 18443 | non-standard, internal hop |
| local SOCKS5 | 11080 | loopback only |

Ports are configurable. Avoid 443, 1194, 51820, 1080, 8080.

## Framing and shaping

- Handshake messages are padded to 128 bytes with random bytes.
- Every frame has a 4-byte masked length and AEAD ciphertext.
- DATA payloads are padded to random buckets (256/512/1024/1500) to blur
  application fingerprints.
- Optional PADDING frames and random inter-frame delays will be added behind a
  config flag.
- No TLS record structure is imitated in v1; the stream looks like random
  binary data on a non-standard port. A TLS-like cover mode is future work.

## Failure model

- Cloud.ru down: client cannot connect; no failover in v1.
- VDSina down: Cloud.ru accepts then closes; client reports timeout.
- PSK mismatch: handshake fails with a generic error; server logs no secrets.
- Replay: server nonce + AEAD sequence numbers reject duplicates within a session.
