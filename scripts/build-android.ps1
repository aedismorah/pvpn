#requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$android = Join-Path $root "android"
$sdk = "C:\Users\Aedis\AppData\Local\Android\Sdk"
$jdk = "C:\Program Files\Eclipse Adoptium\jdk-17.0.21.10002-hotspot"
$ndk = Join-Path $sdk "ndk\27.2.12479018"
$clangArm = Join-Path $ndk "toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android24-clang.cmd"
$clangX64 = Join-Path $ndk "toolchains\llvm\prebuilt\windows-x86_64\bin\x86_64-linux-android24-clang.cmd"

if (-not (Test-Path $jdk)) { throw "JDK 17 not found at $jdk" }
if (-not (Test-Path $sdk)) { throw "Android SDK not found at $sdk" }
if (-not (Test-Path $clangArm)) { throw "NDK clang not found at $clangArm" }
if (-not (Test-Path $clangX64)) { throw "NDK clang not found at $clangX64" }
if (-not (Test-Path (Join-Path $android "app\libs\hev-socks5-tunnel.aar"))) {
    throw "android/app/libs/hev-socks5-tunnel.aar is missing"
}

$env:JAVA_HOME = $jdk
$env:ANDROID_HOME = $sdk
$env:Path = "$env:USERPROFILE\.cargo\bin;C:\msys64\mingw64\bin;$jdk\bin;$sdk\platform-tools;$env:Path"
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = $clangArm
$env:CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER = $clangX64

Push-Location $root
try {
    rustup run stable-x86_64-pc-windows-gnu cargo build --release --target aarch64-linux-android -p pvpn-android
    rustup run stable-x86_64-pc-windows-gnu cargo build --release --target x86_64-linux-android -p pvpn-android
    $jniArm = Join-Path $android "app\src\main\jniLibs\arm64-v8a"
    $jniX64 = Join-Path $android "app\src\main\jniLibs\x86_64"
    New-Item -ItemType Directory -Force $jniArm | Out-Null
    New-Item -ItemType Directory -Force $jniX64 | Out-Null
    Copy-Item "target\aarch64-linux-android\release\libpvpn.so" $jniArm -Force
    Copy-Item "target\x86_64-linux-android\release\libpvpn.so" $jniX64 -Force
} finally {
    Pop-Location
}

Push-Location $android
try {
    & ".\gradlew.bat" --no-daemon assembleDebug
    $apk = Join-Path $android "app\build\outputs\apk\debug\app-debug.apk"
    $dist = Join-Path $root "dist\android"
    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item $apk $dist -Force
    Write-Host "Built: $dist\app-debug.apk" -ForegroundColor Green
} finally {
    Pop-Location
}
