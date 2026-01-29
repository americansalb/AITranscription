#!/bin/bash
# Build the vaak-mcp sidecar for the current platform
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
        BINARY_NAME="vaak-mcp-$TARGET_TRIPLE"
        ;;
    Linux)
        TARGET_TRIPLE="x86_64-unknown-linux-gnu"
        BINARY_NAME="vaak-mcp-$TARGET_TRIPLE"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        TARGET_TRIPLE="x86_64-pc-windows-msvc"
        BINARY_NAME="vaak-mcp-$TARGET_TRIPLE.exe"
        ;;
    *)
        echo "Unknown OS: $OS"
        exit 1
        ;;
esac

echo "Building vaak-mcp for $TARGET_TRIPLE..."

# Build the binary
cd "$TAURI_DIR"
cargo build --bin vaak-mcp --release

# Copy to binaries folder
if [ -f "target/release/vaak-mcp.exe" ]; then
    cp "target/release/vaak-mcp.exe" "$BINARIES_DIR/$BINARY_NAME"
else
    cp "target/release/vaak-mcp" "$BINARIES_DIR/$BINARY_NAME"
fi

echo "Built sidecar: $BINARIES_DIR/$BINARY_NAME"
