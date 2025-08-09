#!/usr/bin/env bash
set -euo pipefail

echo "Building static musl binary for Replicante..."

# Check if musl target is installed
if ! rustup target list --installed | grep -q "x86_64-unknown-linux-musl"; then
    echo "Installing musl target..."
    rustup target add x86_64-unknown-linux-musl
fi

# Set environment variables for static linking
export RUSQLITE_BUNDLED=1
export OPENSSL_NO_VENDOR=0
export RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-static -C link-arg=-static-pie"

# Build the binary
echo "Compiling..."
cargo build --release --target x86_64-unknown-linux-musl

# Verify the binary is static
BINARY="target/x86_64-unknown-linux-musl/release/replicante"

if [ -f "$BINARY" ]; then
    echo ""
    echo "Build complete! Verifying static linking..."
    
    if command -v ldd >/dev/null 2>&1; then
        if ldd "$BINARY" 2>&1 | grep -q "not a dynamic executable"; then
            echo "✓ Binary is statically linked"
        else
            echo "⚠ Warning: Binary may have dynamic dependencies:"
            ldd "$BINARY" || true
        fi
    else
        echo "Note: ldd not available, cannot verify static linking"
    fi
    
    echo ""
    echo "Binary location: $BINARY"
    echo "Binary size: $(du -h "$BINARY" | cut -f1)"
    
    # Test that it runs
    echo ""
    echo "Testing binary..."
    if "$BINARY" --version >/dev/null 2>&1; then
        echo "✓ Binary executes successfully"
    else
        echo "⚠ Binary may have issues running"
    fi
else
    echo "Error: Binary not found at $BINARY"
    exit 1
fi