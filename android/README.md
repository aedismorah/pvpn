# pvpn Android

The Android app uses the Rust `pvpn` client for the encrypted tunnel and
[hev-socks5-tunnel](https://github.com/heiher/hev-socks5-tunnel) for
TUN-to-SOCKS. hev is vendored as `app/libs/hev-socks5-tunnel.aar` (MIT).

```text
app TCP/UDP -> VpnService TUN -> hev-socks5-tunnel
            -> 127.0.0.1:11080 (Rust pvpn client)
            -> relay -> exit -> Internet
```

## Requirements

- Android SDK platform 35 and build-tools 35;
- NDK 27;
- JDK 17;
- Rust targets `aarch64-linux-android` and `x86_64-linux-android`.

## Build

From the repository root:

```powershell
# Windows
.\scripts\build-android.ps1
```

```bash
# macOS / Linux
export ANDROID_HOME="$HOME/Library/Android/sdk"
./scripts/build-android.sh
```

The result is `dist/android/app-debug.apk`.

## Install and use

```bash
adb install -r dist/android/app-debug.apk
```

1. Open pvpn.
2. Enter ingress (`host:port`) and the 64-hex PSK.
3. Start VPN, or start SOCKS5 for apps with their own proxy settings.
4. Accept Android's VPN permission dialog.

The APK has no built-in PSK; enter your own key. The key is not persisted in
plaintext.

## Features

- system-wide VpnService and local SOCKS5 modes;
- split tunneling by applications and by sites/IPs;
- independent White list / Black list mode for each list;
- ping and speed test;
- YouTube traffic diagnostic with a copied log;
- TCP-only mode for stable YouTube/QUIC behavior.

## Notes

- Split settings are read when the VPN starts; restart the VPN after changing
  them.
- IPv4 is routed by the current tunnel; IPv6 is not routed.
- Site rules use resolved IPv4 addresses on Android 13+.
