# Build the scribe-mcp sidecar for the current platform
# This script is run before Tauri build

$ErrorActionPreference = "Stop"

# Get the script directory
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$TauriDir = Join-Path $ScriptDir ".." "src-tauri"
$BinariesDir = Join-Path $TauriDir "binaries"

# Create binaries directory if it doesn't exist
if (-not (Test-Path $BinariesDir)) {
    New-Item -ItemType Directory -Path $BinariesDir | Out-Null
}

# Determine target triple
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
$OS = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription

if ($IsWindows -or $env:OS -eq "Windows_NT") {
    $TargetTriple = "x86_64-pc-windows-msvc"
    $BinaryName = "scribe-mcp-$TargetTriple.exe"
} elseif ($IsMacOS) {
    if ($Arch -eq "Arm64") {
        $TargetTriple = "aarch64-apple-darwin"
    } else {
        $TargetTriple = "x86_64-apple-darwin"
    }
    $BinaryName = "scribe-mcp-$TargetTriple"
} else {
    $TargetTriple = "x86_64-unknown-linux-gnu"
    $BinaryName = "scribe-mcp-$TargetTriple"
}

Write-Host "Building scribe-mcp for $TargetTriple..."

# Build the binary
Push-Location $TauriDir
try {
    cargo build --bin scribe-mcp --release
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo build failed"
    }

    # Copy to binaries folder
    $SourcePath = Join-Path $TauriDir "target" "release" "scribe-mcp.exe"
    if (-not (Test-Path $SourcePath)) {
        $SourcePath = Join-Path $TauriDir "target" "release" "scribe-mcp"
    }

    $DestPath = Join-Path $BinariesDir $BinaryName
    Copy-Item $SourcePath $DestPath -Force

    Write-Host "Built sidecar: $DestPath"
} finally {
    Pop-Location
}
