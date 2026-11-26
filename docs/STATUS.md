# Status

Updated: 2026-11-26

## Working

- `pvpn-core` v1 protocol: PSK + X25519 handshake, HKDF-SHA256, per-direction
  ChaCha20-Poly1305 frames, XOR wire mask, padded DATA frames, stream
  open/result/data/close; UDP relay (`OPEN_UDP`/`UDP_DATA`).
- Windows client: local SOCKS5 (`127.0.0.1:11080` by default), one session per
  SOCKS CONNECT, IPv4/IPv6/domain targets.
- Linux server: multiplexed stream IDs, target connect timeout, SSRF policy that
  rejects loopback/private/link-local/CGNAT/reserved targets by default; UDP
  relay with `OPEN_UDP`/`UDP_DATA` frames.
- `pvpn-relay` on Cloud.ru: blind TCP relay, multiple exits, latency probes,
  live throughput EWMA, health/failover, dynamic config reload.
- Android app (`com.pvpn.app`, ARM64 + x86_64): SOCKS5 foreground service and
  system-wide `VpnService` with TUN-to-SOCKS via hev-socks5-tunnel; JNI bridge,
  fake-IP DNS, start/stop, notification, PSK input.
- Windows desktop UI (`pvpn-desktop`, egui): start/stop tunnel, HTTP + SOCKS
  inbounds, domain split tunneling through a local PAC, system proxy on/off,
  persistent settings, validation, error banner, event log.
- Android UI: branded dark layout, status card, validation, service/native error
  banner with copy/dismiss, remembered server/listener, notification Stop
  actions.
- Real deployment:
  - Cloud.ru `87.242.102.100:9331` runs `pvpn-relay`.
  - VDSina `77.238.226.127:18443` (`pvpn`) and `:18444` (`pvpn2`).
  - Exit IP is `77.238.226.127`.
- Tests: 12/12 (core, PAC, relay config, UDP frame).
- 1 MiB transfer through local two-exit relay: SHA-256 match.
- Real Cloud.ru -> VDSina path: `example.com` 200, exit `77.238.226.127`.
- Live add of a second exit and failover with one exit stopped both return 200.
- Android APK built, installed and run on an API 35 x86_64 emulator; its local
  SOCKS5 produced `example.com` 200 and exit IP `77.238.226.127`.
- Android system VPN verified: `tun0` at `10.255.0.2/32`, Chrome loaded
  `https://api.ipify.org` through the VPN and showed `77.238.226.127`;
  Stop removed `tun0` and closed the SOCKS listener.
- Desktop UI verified on Windows: `11080`/`11081` listeners, PAC on `7892`,
  `chatgpt.com`/`youtube.com` through `127.0.0.1:11081`, `ya.ru` direct,
  HTTP CONNECT 200, exit `77.238.226.127`, Stop cleared `AutoConfigURL`.
- Desktop error path verified: empty/invalid PSK is rejected before spawning,
  the red banner appears, and `Copy error` works.
- Android UI verified on API 35: edge-to-edge header, disabled-button states,
  validation error card for a bad PSK, successful `VPN active - all apps`,
  `tun0` present, Stop removed `tun0`.
- UDP path verified through the real Cloud/VDSina chain: a DNS query sent via
  SOCKS5 UDP ASSOCIATE to `1.1.1.1:53` returned a valid response.
- Android YouTube verified on the physical FNE-NX9: a Shorts video played with
  video and audio through the tunnel. Telegram loaded chats, avatars and
  messages through the same VPN.
- The public Cloud.ru ingress is `9331`. `443` and `9443` were opened in the
  same security group as spare ingress ports; the relay is bound to `9331`.
  The old `xray` service was stopped and disabled so it cannot claim the
  standard port after reboot.
- Android YouTube loaded its full Home feed and played a Shorts video through
  `9331`; the app icon was updated to the supplied glowing-stick artwork.
- Desktop YouTube was fixed by migrating its saved ingress from `:9443` to
  `:9331` and by adding the missing YouTube API/CDN domains
  (`youtubei.googleapis.com`, `gstatic.com`, `ggpht.com`,
  `googleusercontent.com`, `youtube-nocookie.com`) to the PAC split list.
- Android diagnostic: 12/12 TCP requests OK, median TTFB 78–83 ms, max read gap
  59 ms; SOCKS5 UDP DNS 10/10, 53–56 ms, 0% loss. `hev.log` tail showed no
  errors. The test writes `files/youtube-diag.log` and copies it to the
  clipboard.
- Phone TCP-only tunnel measured **177 Mbit/s** for 20 MB through the local
  SOCKS5 path. Cloud.ru now uses BBR/fq; VDSina already did. Android defaults to
  **Prefer TCP**, which disables QUIC and made YouTube loading/playback stable;
  the switch can be turned off for Telegram/VoIP calls.
- Leg measurements: phone -> Cloud ingress 9 ms; Cloud -> VDSina 49 ms avg,
  0% loss; Cloud -> VDSina 20 MB 127 Mbit/s direct / 172 Mbit/s via relay;
  phone -> tunnel 177 Mbit/s. The remaining loading latency is the
  Cloud -> VDSina RTT and per-connection setup, not crypto or packet loss.
- Android Ping button verified on the phone: `Ingress: 9 ms`, `Tunnel: 156 ms`.
  Speed-test UI downloads 5 MB through the local SOCKS5 listener.

## Evidence

```text
cargo test
  12 passed; 0 failed

/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml list
  vdsina-nl-1  77.238.226.127:18443  enabled=true
  vdsina-nl-2  77.238.226.127:18444  enabled=true

/opt/pvpn/pvpn-relay ... probe
  vdsina-nl-1  45.3 ms
  vdsina-nl-2  49.1 ms

curl --socks5-hostname 127.0.0.1:11082 http://example.com/
  200

systemctl stop pvpn2; curl ... http://example.com/
  200   (relay failed over to nl-1)

adb install -r dist/android/app-debug.apk
adb shell am start -n com.pvpn.app/.MainActivity
# enter 87.242.102.100:9331 + PSK, Start
adb forward tcp:11180 tcp:11080
curl --socks5-hostname 127.0.0.1:11180 http://example.com/
  200
curl --socks5-hostname 127.0.0.1:11180 http://api.ipify.org
  77.238.226.127

# Android system VPN
Start VPN -> tun0 10.255.0.2/32
Chrome https://api.ipify.org -> 77.238.226.127
Stop VPN -> tun0 removed, 11080 closed

pvpn-desktop.exe
  Start tunnel -> 11080/11081 listening
  Enable system proxy -> AutoConfigURL=http://127.0.0.1:7892/proxy.pac
  chatgpt.com/youtube.com -> 127.0.0.1:11081
  ya.ru -> direct
  HTTP CONNECT example.com -> 200
  Stop -> AutoConfigURL removed, listeners closed
```

## Not done yet

- Android VPN has no IPv6 routing; UDP/QUIC is supported through SOCKS5 UDP
  ASSOCIATE and pvpn UDP frames.
- Windows split tunneling is PAC/system-proxy based; no per-process routing.
- Windows UI stores the PSK in `%APPDATA%\pvpn\ui.toml` in plaintext.
- Android builds ARM64 and x86_64; no armeabi-v7a APK.
- Throughput is sampled at connection close; no live mid-transfer update.
- No mid-session exit migration (would break the end-to-end stream).
- Cover traffic, idle timeouts, handshake rate limiting.
- Per-exit PSKs (all exits currently share one PSK).
- Server firewall restriction of 18443/18444 to the Cloud.ru IP.
- Protocol audit and traffic-classification measurements.

## Next milestone: IPv6 and traffic shaping

1. Add IPv6 TUN routing (`fc00::` address/route, hev dual-stack).
2. Add per-app split tunneling on Android and per-process routing on Windows.
3. Add cover traffic / padding policy and measure DPI classification.
4. Add release signing, versioning and an update path for the APK/EXE.
