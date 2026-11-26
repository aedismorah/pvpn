#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
NDK="${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)}"
HOST_TAG="darwin-x86_64"
if [[ "$(uname -s)" == "Linux" ]]; then
  HOST_TAG="linux-x86_64"
fi
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/$HOST_TAG/bin"
CLANG_ARM="$TOOLCHAIN/aarch64-linux-android24-clang"
CLANG_X64="$TOOLCHAIN/x86_64-linux-android24-clang"

if [[ ! -x "$CLANG_ARM" || ! -x "$CLANG_X64" ]]; then
  echo "Android NDK clang not found under $TOOLCHAIN" >&2
  echo "Set ANDROID_HOME or ANDROID_NDK_HOME." >&2
  exit 1
fi
if [[ ! -f "$ROOT/android/app/libs/hev-socks5-tunnel.aar" ]]; then
  echo "android/app/libs/hev-socks5-tunnel.aar is missing" >&2
  exit 1
fi

export ANDROID_HOME
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG_ARM"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$CLANG_X64"

if [[ ! -f "$ROOT/android/local.properties" ]]; then
  printf 'sdk.dir=%s\n' "$ANDROID_HOME" > "$ROOT/android/local.properties"
fi

cd "$ROOT"
rustup target add aarch64-linux-android x86_64-linux-android
cargo build --release --target aarch64-linux-android -p pvpn-android
cargo build --release --target x86_64-linux-android -p pvpn-android

mkdir -p android/app/src/main/jniLibs/arm64-v8a android/app/src/main/jniLibs/x86_64
cp target/aarch64-linux-android/release/libpvpn.so android/app/src/main/jniLibs/arm64-v8a/
cp target/x86_64-linux-android/release/libpvpn.so android/app/src/main/jniLibs/x86_64/

cd android
./gradlew --no-daemon assembleDebug
mkdir -p "$ROOT/dist/android"
cp app/build/outputs/apk/debug/app-debug.apk "$ROOT/dist/android/app-debug.apk"
echo "Built: $ROOT/dist/android/app-debug.apk"
