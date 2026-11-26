#requires -Version 5.1
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;C:\msys64\mingw64\bin;$env:Path"

Push-Location $root
try {
    rustup run stable-x86_64-pc-windows-gnu cargo build --release --target x86_64-pc-windows-gnu
    $dist = Join-Path $root "dist\windows"
    New-Item -ItemType Directory -Force $dist | Out-Null
    Copy-Item "target\x86_64-pc-windows-gnu\release\pvpn-server.exe" $dist -Force
    Copy-Item "target\x86_64-pc-windows-gnu\release\pvpn-client.exe" $dist -Force
    Copy-Item "target\x86_64-pc-windows-gnu\release\pvpn-relay.exe" $dist -Force
    Copy-Item "target\x86_64-pc-windows-gnu\release\pvpn-desktop.exe" $dist -Force
    Copy-Item "server.toml.example" $dist -Force -ErrorAction SilentlyContinue
    Copy-Item "client.toml.example" $dist -Force -ErrorAction SilentlyContinue
    Write-Host "Built: $dist" -ForegroundColor Green
} finally {
    Pop-Location
}
