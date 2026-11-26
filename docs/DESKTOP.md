# Windows desktop UI

`pvpn-desktop` is an egui/eframe application for Windows. It wraps the same
Rust client as the CLI and adds:

- Start / Stop tunnel;
- Enable / Disable Windows system proxy;
- domain-based split tunneling;
- white-list / black-list split tunneling modes;
- persistent settings in `%APPDATA%\pvpn\ui.toml`;
- status chips, an error banner, and an event log;
- input validation for ingress, PSK and every local port.

While the tunnel is running the connection fields are locked, so a running
session cannot silently drift from the saved config. Stop the tunnel to edit.

![Desktop UI](desktop-ui.png)

It is a **system-proxy/PAC** app, not a TUN VPN. Apps that ignore the Windows
system proxy (some games, custom network stacks) will not use the tunnel; for
those, configure the local SOCKS5 `127.0.0.1:11080` manually.

## Ports

| Purpose | Default |
| --- | --- |
| SOCKS5 | `127.0.0.1:11080` |
| HTTP proxy (used by PAC) | `127.0.0.1:11081` |
| PAC server | `127.0.0.1:7892` |

`7892` is used to avoid the legacy `proxyctl` PAC server on `7891`.

## How split tunneling works

The split page has two list modes:

```text
White list:
  listed domain or subdomain -> PROXY 127.0.0.1:11081
  everything else            -> DIRECT

Black list:
  listed domain or subdomain -> DIRECT
  everything else            -> PROXY 127.0.0.1:11081
```

Localhost, RFC1918, link-local and `.local` names always go direct. Domain
checkboxes are multi-select; **Remove selected** deletes all checked rules.

Windows fetches the PAC from `http://127.0.0.1:7892/proxy.pac` through the
`AutoConfigURL` value in the current user's Internet Settings. The UI calls
`InternetSetOptionW` after changing it.

## Run

```powershell
.\dist\windows\pvpn-desktop.exe
```

1. Ingress: `87.242.102.100:9331`.
2. PSK: 64 hex characters.
3. Press **Start tunnel**.
4. Press **Enable system proxy**.
5. Open **Split tunneling** (or press `F2`) to manage domains; use **Back** or
   `Esc` to return. The changes apply immediately.

## Connection tests

The main page has a **Connection test** card:

- **Ping** measures TCP connect time to the ingress and, when the tunnel is
  running, an HTTP CONNECT through the local proxy.
- **Speed test** downloads 10 MB through the local HTTP proxy
  (`cachefly`, with a fallback host) and reports Mbps, size and duration.
  The tunnel must be running.



- Start opened `11080` and `11081`.
- Enable wrote `AutoConfigURL=http://127.0.0.1:7892/proxy.pac?...`.
- `GetSystemWebProxy()`:
  - `chatgpt.com` → `127.0.0.1:11081`
  - `youtube.com` → `127.0.0.1:11081`
  - `ya.ru` → direct
- HTTP CONNECT through `11081`: `example.com` 200.
- Plain HTTP through `11081`: exit IP `77.238.226.127`.
- Stop cleared `AutoConfigURL` and closed both listeners.

## Errors

- Invalid PSK, host:port or local IP:port is rejected before spawning threads.
- A tunnel thread that exits is surfaced in the error banner and stopped cleanly.
- A failed PAC server is reported instead of silently leaving system proxy broken.
- Registry changes use `reg.exe`; failures are shown with the exact Windows error.
- `Copy error` and `Copy log` make support/debugging easier.
- `Disable system proxy` is attempted on Stop and on window close.

## Limitations

- Domain split tunneling only applies to WinINET/system-proxy-aware apps.
- Telegram Desktop ignores the PAC; configure its own SOCKS5 proxy
  `127.0.0.1:11080` (see [TELEGRAM.md](TELEGRAM.md)).
- PSK is stored in `%APPDATA%\pvpn\ui.toml` in plaintext; protect the profile or
  clear the field.
- No per-process routing and no TUN adapter yet.
- No autostart/tray integration yet.
