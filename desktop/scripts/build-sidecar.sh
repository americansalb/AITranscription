#!/bin/bash
# Build the scribe-mcp sidecar for the current platform
# This script is run before Tauri build

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TAURI_DIR="$SCRIPT_DIR/../src-tauri"
BINARIES_DIR="$TAURI_DIR/binaries"

# Create binaries directory if it doesn't exist
mkdir -p "$BINARIES_DIR"

# Determine target triple
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        if [ "$ARCH" = "arm64" ]; then
            TARGET_TRIPLE="aarch64-apple-darwin"
        else
            TARGET_TRIPLE="x86_64-apple-darwin"
        fi
        BINARY_NAME="scribe-mcp-$TARGET_TRIPLE"
        ;;
    Linux)
        TARGET_TRIPLE="x86_64-unknown-linux-gnu"
        BINARY_NAME="scribe-mcp-$TARGET_TRIPLE"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        TARGET_TRIPLE="x86_64-pc-windows-msvc"
        BINARY_NAME="scribe-mcp-$TARGET_TRIPLE.exe"
        ;;
    *)
        echo "Unknown OS: $OS"
        exit 1
        ;;
esac

echo "Building scribe-mcp for $TARGET_TRIPLE..."

# Build the binary
cd "$TAURI_DIR"
cargo build --bin scribe-mcp --release

# Copy to binaries folder
if [ -f "target/release/scribe-mcp.exe" ]; then
    cp "target/release/scribe-mcp.exe" "$BINARIES_DIR/$BINARY_NAME"
else
    cp "target/release/scribe-mcp" "$BINARIES_DIR/$BINARY_NAME"
fi

echo "Built sidecar: $BINARIES_DIR/$BINARY_NAME"
