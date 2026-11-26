# pvpn

Experimental Rust tunnel for Windows, Android and Linux with a custom encrypted
transport. The traffic path is:

```text
Windows / Android  ->  Cloud.ru relay (RU)  ->  VDSina exit (NL)  ->  Internet
```

The relay forwards opaque TCP; the cryptographic endpoint is the exit server.
The wire format is a length-framed AEAD protocol with a random handshake and an
XOR mask layer. It is not VLESS, WireGuard, SOCKS5 or TLS on the wire.

> Research prototype. Custom cryptography is not a substitute for an audited
> protocol. Use at your own risk.

## Repository layout

```text
crates/pvpn-core       protocol, crypto, frames
crates/pvpn-client     client/SOCKS5 engine
crates/pvpn-server     exit server
crates/pvpn-relay      Cloud.ru relay and exit selection
crates/pvpn-desktop    Windows egui desktop app
crates/pvpn-android    JNI bridge for the Android app
android/               Android application and VpnService
assets/brand/          app icons
scripts/               build helpers
```

## Build

Requirements:

- Rust stable;
- Windows desktop: `x86_64-pc-windows-gnu` target, MinGW `windres`;
- Android: JDK 17, Android SDK platform 35, NDK 27, targets
  `aarch64-linux-android` and `x86_64-linux-android`.

### Windows desktop and CLI

From PowerShell with MSYS2 MinGW on `PATH`:

```powershell
rustup target add x86_64-pc-windows-gnu
.\scripts\build-windows.ps1
```

Outputs:

```text
dist/windows/pvpn-desktop.exe
dist/windows/pvpn-client.exe
dist/windows/pvpn-server.exe
dist/windows/pvpn-relay.exe
```

The desktop app is Windows-specific: it uses WinINet/`AutoConfigURL` for the
system proxy and PAC-based split tunneling.

### Android (Windows)

```powershell
.\scripts\build-android.ps1
```

### Android (macOS / Linux)

Set the SDK path if it is not in the default location, then run:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"   # macOS
./scripts/build-android.sh
```

Output:

```text
dist/android/app-debug.apk
```

The build compiles `libpvpn.so` for ARM64 and x86_64, copies the libraries into
the Gradle project and produces a debug APK.

The APK does not contain a default PSK. Enter the 64-hex key on first launch.
The key is not persisted in plaintext by the app.

### Linux server / relay

```bash
cargo build --release -p pvpn-server -p pvpn-client -p pvpn-relay
```

Use `server.toml.example` and `client.toml.example` as templates. Keep real
keys out of git.

## Desktop

```powershell
.\dist\windows\pvpn-desktop.exe
```

Start the tunnel, then enable the system proxy. Split tunneling is PAC/domain
based; Telegram Desktop and apps that ignore the Windows proxy need the local
SOCKS5 listener. See `docs/DESKTOP.md`.

## Android

Install the APK, enter ingress and PSK, then start the VPN or local SOCKS5.

The app includes:

- system-wide `VpnService` through hev-socks5-tunnel;
- SOCKS5-only foreground mode;
- split tunneling by sites/IPs and by applications;
- White list / Black list mode for each list;
- Ping, speed test and YouTube diagnostic.

See `android/README.md` for details.

## iOS

The repository does not contain an iOS target yet. An iOS client needs a
separate Swift `NEPacketTunnelProvider` extension and Apple Network Extension
entitlements; it cannot be built from this Windows-oriented workspace without a
macOS/Xcode signing setup.

## Documents

- `docs/ARCHITECTURE.md`
- `docs/PROTOCOL.md`
- `docs/RELAY.md`
- `docs/DESKTOP.md`
- `docs/THREAT_MODEL.md`
