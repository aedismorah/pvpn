# Threat model

## Adversary

- ISP / TSPU on the Russian access network.
- Passive traffic analysis and active probing.
- Machine-learning protocol classifiers using ports, packet sizes, timing,
  entropy, TLS fingerprints and connection patterns.
- Adversary can block a destination IP/port or throttle bulk flows.

The cloud ingress (Cloud.ru) is trusted for availability but **not** for
confidentiality: it only sees opaque TCP bytes and the VDSina IP/port.

## Assets

- payload contents and destinations;
- PSK and derived session keys;
- client identity / metadata (source IP, timing, volume).

## Properties

| Property | v1 |
| --- | --- |
| Payload confidentiality | ChaCha20-Poly1305 end-to-end to VDSina |
| Payload integrity | AEAD per frame |
| Handshake authentication | PSK + HMAC + X25519 |
| Forward secrecy | yes, ephemeral X25519 per session |
| Replay resistance | server nonce + per-frame sequence |
| Destination privacy from Cloud.ru | yes (Cloud.ru forwards bytes) |
| Metadata privacy from ISP | partial (volume/timing still visible) |
| Port/protocol fingerprint resistance | non-standard ports, random hello, padded frames |
| Resistance to a determined ML classifier | not proven; requires measurement |
| Audited crypto | no |

## Non-goals / limitations

- Not a replacement for Tor anonymity.
- Not resistant to a global active adversary who blocks all unknown protocols.
- Not resistant to endpoint compromise.
- No cover traffic in v1; large download/upload patterns remain visible.
- The XOR mask is cosmetic; it does not add cryptographic strength.
- Custom protocols can contain subtle cryptographic mistakes. The design uses
  standard primitives and fixed-size messages to reduce risk, but it has not been
  audited.

## Operational rules

- Generate a unique 32-byte PSK per deployment, store it with 0600 permissions.
- Never reuse a client nonce; use `OsRng`.
- Rotate the PSK if a device is lost.
- Keep server and client clocks roughly correct for logs, but do not use time for
  authentication.
- Do not log payloads, PSKs, raw handshakes or target addresses at info level.
- Rate-limit handshakes per source IP on the server.
- Monitor handshake failure rates; a spike may indicate active probing.
