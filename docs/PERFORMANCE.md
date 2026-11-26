# Desktop proxy throughput

Measured: 2026-11-26, from the Windows host in Russia through:

```text
pvpn client (local HTTP proxy) -> Cloud.ru relay:443 -> VDSina:18443 -> Cloudflare
```

The client was the same `pvpn-client` library used by `pvpn-desktop`; the local
listener was `127.0.0.1:11083` (HTTP CONNECT) so the benchmark did not disturb
the normal desktop UI ports. Both Cloud.ru exits currently point at the same
VDSina host, so this is not an independent multi-exit comparison.

Endpoint: `https://speed.cloudflare.com/__down?bytes=...` and `/__up`.
TLS verification is disabled for the benchmark only because the machine's curl
schannel stack has a certificate issue; Python used an unverified context.

## 20 MB download, 5 runs

| Run | Direct | pvpn proxy |
| --- | ---: | ---: |
| 1 | 93.60 Mbps | 73.70 Mbps |
| 2 | 72.73 Mbps | 94.57 Mbps |
| 3 | 84.60 Mbps | 111.20 Mbps |
| 4 | 126.22 Mbps | 72.48 Mbps |
| 5 | 102.98 Mbps | 54.66 Mbps |
| **Median** | **93.60 Mbps** | **73.70 Mbps** |
| Mean | 96.03 Mbps | 81.32 Mbps |
| Range | 72.7–126.2 | 54.7–111.2 |

The proxy median is about **79% of the direct median** in this sample, but the
range is wide. The proxy sometimes beats direct because Cloudflare/network
conditions fluctuate and the exit path is different.

## First-byte latency

For the 20 MB runs, time from starting the request to the first response byte:

- direct: 194–228 ms, median **209 ms**;
- pvpn proxy: 376–558 ms, median **420 ms**.

The proxy adds roughly **0.2 s** to TTFB (proxy TCP + SOCKS/HTTP CONNECT + TLS
handshake + Cloud→VDSina RTT). It does not add 0.2 s per downloaded chunk.

## Upload

5 MB POST to `/__up`:

| Path | Samples | Median |
| --- | --- | ---: |
| Direct (5 runs) | 101.25, 69.53, 98.17, 66.71, 67.96 Mbps | **69.53 Mbps** |
| pvpn proxy (2 runs) | 46.22, 23.92 Mbps | **35.07 Mbps** |

Upload is noticeably more variable than download; the proxy generally reached
**24–46 Mbps** on the 5 MB test. More proxy samples are needed for a tighter
number.

2 MB uploads were noisier (direct ~43–46 Mbps; proxy 3.9–18.6 Mbps), so the
5 MB numbers are more representative.

## SOCKS5 path (Telegram-style)

The desktop UI also exposes SOCKS5 on `127.0.0.1:11080`. 20 MB through that
listener:

| Run | Throughput |
| --- | ---: |
| 1 | 62.42 Mbps |
| 2 | 68.78 Mbps |
| 3 | 80.64 Mbps |
| **Median** | **68.78 Mbps** |

First byte after the SOCKS connection was established: 89–118 ms. This is the
same client/protocol path Telegram uses; it confirms the local SOCKS listener is
not the bottleneck.

## Where the latency is

Measured legs:

| Leg | Result |
| --- | --- |
| Phone -> Cloud TCP ingress | 9 ms, no failures |
| Cloud -> VDSina | 49.0 ms avg, 0% packet loss, 0.37 ms jitter |
| Cloud -> VDSina, 20 MB | 127 Mbit/s direct |
| Cloud -> relay -> VDSina, 20 MB | 172 Mbit/s |
| Phone -> pvpn tunnel, 20 MB | 177 Mbit/s |
| VDSina -> Cloudflare | HTTP 200, low RTT |

Throughput is not the bottleneck on either leg. The visible loading latency is
mostly the **~49 ms Cloud -> VDSina round trip** plus per-connection target
setup. This is why QUIC/QV churn mattered more than crypto: every new QUIC
association pays that RTT again.

The next meaningful latency improvement would be session multiplexing (several
SOCKS connections over one pvpn session) or an exit closer to the phone. Do not
optimise ChaCha/XOR for throughput: 177 Mbit/s is already far above the YouTube
bitrate.

## TCP-only / QUIC experiment (phone)

The Android VPN has a **Prefer TCP (disable QUIC/UDP)** switch, enabled by
default. With QUIC disabled, YouTube's full feed and video playback were
stable; QUIC over the SOCKS5 UDP relay showed frequent session churn and was
less predictable. Telegram messages and files still use TCP; disable the switch
only if you need Telegram/VoIP calls or another UDP application.

Phone-to-Cloud TCP-only tunnel, 20 MB from Cloudflare through the phone's local
SOCKS5 listener forwarded over ADB:

```text
177 Mbit/s (1.13 s for 20 MB)
```

The raw tunnel is not the bottleneck at that point; page-load latency is
dominated by the Cloud -> VDSina -> Google path and per-connection target setup.

## Cloud.ru BBR

Cloud.ru's kernel had BBR available but loaded `cubic` with `fq_codel`.
`tcp_bbr` is now loaded, with:

```text
net.core.default_qdisc=fq
net.ipv4.tcp_congestion_control=bbr
net.ipv4.tcp_slow_start_after_idle=0
net.ipv4.tcp_mtu_probing=1
```

VDSina already used BBR/fq. The settings are persisted in
`/etc/sysctl.d/99-pvpn.conf` and `/etc/modules-load.d/pvpn-bbr.conf`.

## YouTube diagnostics

The Android **YouTube test** writes `files/youtube-diag.log`: three rounds of
`www.youtube.com`, `m.youtube.com`, `i.ytimg.com`, `www.gstatic.com`, connect/
TLS/TTFB/total timings, read-gap stalls, ten SOCKS5 UDP DNS probes and a
`hev.log` tail summary. Latest run: 12/12 TCP requests OK, median TTFB 78–83 ms,
UDP DNS 10/10 at 53–56 ms, no errors.

## Telegram note

Telegram can retain long-lived sockets through a proxy that was removed from its
UI. In this session it kept **59 established connections to the old
`127.0.0.1:10808`** and only a few to `11080`, which made transfers look far
slower than the proxy benchmark. After removing the old proxy entry and
restarting Telegram, all established Telegram connections were
`127.0.0.1:11080`.

If Telegram throughput is low, check:

```powershell
$tg = Get-Process Telegram | Select-Object -First 1
Get-NetTCPConnection -OwningProcess $tg.Id -State Established |
  Group-Object RemoteAddress,RemotePort
```

If `10808` (or any non-`11080` proxy) appears, remove it and fully restart
Telegram.

## What this means

- Typical desktop browsing/downloads: roughly **55–110 Mbps** through the proxy
  on this host/path, median ~74 Mbps.
- The path is fast enough for Telegram files, YouTube and normal desktop use.
- Uploads are slower and more variable, roughly **24–46 Mbps** in this test.
- A single TCP connection is used per proxied session. The relay chooses an exit
  per connection and does not migrate a live download, so a long transfer stays
  on the exit chosen when it started.
- Results depend on Cloud.ru/VDSina link load, Cloudflare, Wi-Fi and the Windows
  host. Add independent VDSina hosts to compare exits meaningfully.

The benchmark is a measurement of this deployment, not a guaranteed speed.
