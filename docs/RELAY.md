# Smart relay (Cloud.ru)

`pvpn-relay` is a blind TCP relay on the Russian ingress. It terminates no
crypto: it selects a VDSina exit for each new client TCP connection and copies
bytes in both directions. The client's `pvpn` session still ends at the chosen
VDSina server.

## Selection

Each exit keeps runtime state:

- EWMA TCP connect latency from periodic probes;
- EWMA throughput measured from finished relayed connections;
- health (3 consecutive probe/connect failures marks it unhealthy).

Per new connection the relay computes:

```text
score = (latency_ms + 100000 / kbps + jitter) / weight
```

and picks the lowest score. Unhealthy exits are skipped unless all exits are
unhealthy. Throughput is sampled when a connection closes, so long-lived
downloads influence the ranking after they finish. Selection happens at
connection start; it never migrates a live session.

## Config

`/opt/pvpn/relay.toml` on Cloud.ru:

```toml
listen = "0.0.0.0:9331"
connect_timeout_ms = 4000
probe_interval_secs = 15

[[servers]]
name = "vdsina-nl-1"
addr = "77.238.226.127:18443"
enabled = true
weight = 1.0

[[servers]]
name = "vdsina-nl-2"
addr = "77.238.226.127:18444"
enabled = true
weight = 1.0
```

The relay watches the file every 2 seconds and reloads only the server list;
`listen`/timeouts require a restart.

## CLI

Run on Cloud.ru:

```bash
/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml list
/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml add nl-3 203.0.113.10:18443
/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml disable nl-2
/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml enable nl-2
/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml remove nl-3
/opt/pvpn/pvpn-relay --config /opt/pvpn/relay.toml probe
```

`add`/`remove`/`disable` take effect without restarting the relay. The CLI writes
the config atomically, then the watcher reloads it.

## Deploy

```bash
# on Cloud.ru
sudo install -m 0755 pvpn-relay /opt/pvpn/pvpn-relay
sudo install -m 0600 relay.toml /opt/pvpn/relay.toml
sudo install -m 0644 pvpn-relay.service /etc/systemd/system/pvpn-relay.service
sudo systemctl daemon-reload
sudo systemctl disable --now pvpn-forward   # old socat forwarder, if present
sudo systemctl disable --now xray           # old 443 proxy; keep the port free
sudo systemctl enable --now pvpn-relay
```

Enable BBR on the ingress (the Cloud kernel may need the module first):

```bash
sudo modprobe tcp_bbr
echo tcp_bbr | sudo tee /etc/modules-load.d/pvpn-bbr.conf
sudo cp 99-pvpn.conf /etc/sysctl.d/99-pvpn.conf
sudo sysctl -w net.ipv4.tcp_congestion_control=bbr
sudo sysctl -w net.core.default_qdisc=fq
```

VDSina already runs BBR/fq. The load test with BBR was 177 Mbit/s through the
phone tunnel; the remaining delay is the ~49 ms Cloud -> VDSina RTT.

`9331` is above 1024, so the unprivileged `aedis` service can bind it without
capabilities. The old `xray`/`pvpn-forward` services stay disabled. `443` and
`9443` are also open in the Cloud security group as spare ingress ports; the
active relay uses `9331`.

## Current deployment

```text
Cloud.ru 87.242.102.100:9331
  -> 77.238.226.127:18443 (vdsina-nl-1, pvpn)
  -> 77.238.226.127:18444 (vdsina-nl-2, pvpn2)
```

Both exits are on the same VDSina host and share a PSK; the second listener is a
test of multi-exit selection. Add real independent VDSina hosts with the same
CLI and they will participate immediately.
